package com.workbench.apkupdater

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

private const val RELEASE_PREFIX = "https://github.com/datstarkey/workbench/releases/download/"
private val REDIRECT_HOSTS = setOf("release-assets.githubusercontent.com", "objects.githubusercontent.com")
private const val MAX_REDIRECTS = 5
private const val MAX_APK_BYTES = 200L * 1024 * 1024
private const val PROGRESS_INTERVAL_MS = 200L

class ApkFileProvider : FileProvider()

@InvokeArg
class DownloadArgs {
  lateinit var url: String
  lateinit var sha256Url: String
  var onProgress: Channel? = null
}

/**
 * Authenticity comes from Android itself: an APK signed with a different key than the
 * installed app is refused as an update. The SHA-256 ships in the same release as the APK,
 * so it only guards against a corrupt or truncated download.
 */
@TauriPlugin
class ApkUpdaterPlugin(private val activity: Activity) : Plugin(activity) {
  private val executor = Executors.newSingleThreadExecutor()
  private val busy = AtomicBoolean(false)

  @Command
  fun downloadAndInstall(invoke: Invoke) {
    val args = invoke.parseArgs(DownloadArgs::class.java)
    if (!args.url.startsWith(RELEASE_PREFIX) || !args.sha256Url.startsWith(RELEASE_PREFIX)) {
      invoke.reject("Refusing to download an update from outside the Workbench GitHub releases")
      return
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
      !activity.packageManager.canRequestPackageInstalls()
    ) {
      val settings = Intent(
        Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
        Uri.parse("package:${activity.packageName}")
      )
      activity.runOnUiThread { activity.startActivity(settings) }
      invoke.resolve(status("needs_permission"))
      return
    }
    if (!busy.compareAndSet(false, true)) {
      invoke.reject("An update is already downloading")
      return
    }
    executor.execute {
      try {
        val expected = parseSha256(String(fetchBytes(args.sha256Url, 1024)))
        val apk = download(args.url, expected, args.onProgress)
        val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.apkupdater", apk)
        val install = Intent(Intent.ACTION_VIEW)
          .setDataAndType(uri, "application/vnd.android.package-archive")
          .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        activity.runOnUiThread { activity.startActivity(install) }
        invoke.resolve(status("installing"))
      } catch (e: Exception) {
        invoke.reject(e.message ?: e.toString())
      } finally {
        busy.set(false)
      }
    }
  }

  private fun status(value: String): JSObject {
    val result = JSObject()
    result.put("status", value)
    return result
  }

  private fun parseSha256(body: String): String {
    val hex = body.trim().split(Regex("\\s+")).firstOrNull()?.lowercase() ?: ""
    if (!hex.matches(Regex("[0-9a-f]{64}"))) throw IOException("Malformed checksum file")
    return hex
  }

  /** Opens [url], following redirects only to GitHub's release asset storage. */
  private fun open(url: String): HttpURLConnection {
    var current = URL(url)
    for (hop in 0..MAX_REDIRECTS) {
      val conn = current.openConnection() as HttpURLConnection
      conn.instanceFollowRedirects = false
      conn.connectTimeout = 15_000
      conn.readTimeout = 30_000
      val code = conn.responseCode
      if (code in 300..399) {
        val location = conn.getHeaderField("Location")
        conn.disconnect()
        val next = URL(current, location ?: throw IOException("Redirect without a location"))
        if (next.protocol != "https" || next.host !in REDIRECT_HOSTS) {
          throw IOException("Unexpected redirect to ${next.host}")
        }
        current = next
        continue
      }
      if (code != HttpURLConnection.HTTP_OK) {
        conn.disconnect()
        throw IOException("Download failed (HTTP $code)")
      }
      return conn
    }
    throw IOException("Too many redirects")
  }

  private fun fetchBytes(url: String, limit: Int): ByteArray {
    val conn = open(url)
    try {
      val bytes = conn.inputStream.use { it.readBytes() }
      if (bytes.size > limit) throw IOException("Checksum file is too large")
      return bytes
    } finally {
      conn.disconnect()
    }
  }

  private fun download(url: String, expectedSha256: String, progress: Channel?): File {
    val dir = File(activity.cacheDir, "apk-updates").apply { mkdirs() }
    val file = File(dir, "workbench-update.apk")
    val conn = open(url)
    val total = conn.contentLengthLong
    if (total > MAX_APK_BYTES) {
      conn.disconnect()
      throw IOException("Update is unexpectedly large")
    }
    val digest = MessageDigest.getInstance("SHA-256")
    try {
      var downloaded = 0L
      var lastReport = 0L
      conn.inputStream.use { input ->
        file.outputStream().use { output ->
          val buffer = ByteArray(64 * 1024)
          while (true) {
            val read = input.read(buffer)
            if (read < 0) break
            downloaded += read
            if (downloaded > MAX_APK_BYTES) throw IOException("Update is unexpectedly large")
            digest.update(buffer, 0, read)
            output.write(buffer, 0, read)
            val now = System.currentTimeMillis()
            if (progress != null && now - lastReport >= PROGRESS_INTERVAL_MS) {
              lastReport = now
              val event = JSObject()
              event.put("downloaded", downloaded)
              event.put("total", total)
              progress.send(event)
            }
          }
        }
      }
      val actual = digest.digest().joinToString("") { "%02x".format(it) }
      if (actual != expectedSha256) throw IOException("Checksum mismatch, the download was discarded")
      return file
    } catch (e: Exception) {
      file.delete()
      throw e
    } finally {
      conn.disconnect()
    }
  }
}
