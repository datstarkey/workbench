package com.workbench.apkupdater

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
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
private val SHA256_HEX = Regex("[0-9a-f]{64}")
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

@InvokeArg
class InstallDownloadedArgs {
  lateinit var sha256: String
}

/**
 * Authenticity comes from Android itself: an APK signed with a different key than the
 * installed app is refused as an update. The SHA-256 ships in the same release as the APK,
 * so it only guards against a corrupt or truncated download.
 *
 * A verified APK is kept as `apk-updates/<sha256>.apk`, so a dropped installer launch or a
 * retry reuses it instead of downloading again.
 */
@TauriPlugin
class ApkUpdaterPlugin(private val activity: Activity) : Plugin(activity) {
  private val executor = Executors.newSingleThreadExecutor()
  private val busy = AtomicBoolean(false)
  private val updatesDir get() = File(activity.cacheDir, "apk-updates")

  @Command
  fun downloadAndInstall(invoke: Invoke) {
    val args = invoke.parseArgs(DownloadArgs::class.java)
    if (!args.url.startsWith(RELEASE_PREFIX) || !args.sha256Url.startsWith(RELEASE_PREFIX)) {
      invoke.reject("Refusing to download an update from outside the Workbench GitHub releases")
      return
    }
    if (!hasInstallPermission(invoke)) return
    runExclusive(invoke) {
      val expected = parseSha256(String(fetchBytes(args.sha256Url, 1024)))
      val apk = File(updatesDir.apply { mkdirs() }, "$expected.apk")
      updatesDir.listFiles()?.filter { it != apk }?.forEach { it.delete() }
      if (!apk.exists() || sha256Of(apk) != expected) download(args.url, apk, expected, args.onProgress)
      launchInstaller(invoke, apk, expected)
    }
  }

  @Command
  fun installDownloaded(invoke: Invoke) {
    val sha256 = invoke.parseArgs(InstallDownloadedArgs::class.java).sha256
    if (!sha256.matches(SHA256_HEX)) {
      invoke.reject("Malformed checksum")
      return
    }
    if (!hasInstallPermission(invoke)) return
    runExclusive(invoke) {
      val apk = File(updatesDir, "$sha256.apk")
      if (!apk.exists() || sha256Of(apk) != sha256) {
        apk.delete()
        throw IOException("The downloaded update is gone, download it again")
      }
      launchInstaller(invoke, apk, sha256)
    }
  }

  /** Called once the running app is current, so an installed update's APK doesn't linger. */
  @Command
  fun clearDownloads(invoke: Invoke) {
    if (!busy.get()) updatesDir.listFiles()?.forEach { it.delete() }
    invoke.resolve()
  }

  private fun runExclusive(invoke: Invoke, work: () -> Unit) {
    if (!busy.compareAndSet(false, true)) {
      invoke.reject("An update is already downloading")
      return
    }
    executor.execute {
      try {
        work()
      } catch (e: Exception) {
        invoke.reject(e.message ?: e.toString())
      } finally {
        busy.set(false)
      }
    }
  }

  private fun hasInstallPermission(invoke: Invoke): Boolean {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O ||
      activity.packageManager.canRequestPackageInstalls()
    ) {
      return true
    }
    val settings = Intent(
      Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
      Uri.parse("package:${activity.packageName}")
    )
    activity.runOnUiThread {
      if (startActivity(settings)) {
        invoke.resolve(status("needs_permission"))
      } else {
        invoke.reject("This device has no setting to allow app installs")
      }
    }
    return false
  }

  /**
   * Android 10+ silently drops activity starts from the background, so a download that
   * finishes while the app isn't resumed hands back `ready_to_install` for the UI to offer.
   */
  private fun launchInstaller(invoke: Invoke, apk: File, sha256: String) {
    val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.apkupdater", apk)
    val install = Intent(Intent.ACTION_VIEW)
      .setDataAndType(uri, "application/vnd.android.package-archive")
      .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    activity.runOnUiThread {
      val resumed = (activity as? LifecycleOwner)?.lifecycle?.currentState
        ?.isAtLeast(Lifecycle.State.RESUMED) ?: true
      when {
        !resumed -> invoke.resolve(status("ready_to_install").put("sha256", sha256))
        startActivity(install) -> invoke.resolve(status("installing"))
        else -> invoke.reject("This device has no installer for APK files")
      }
    }
  }

  // resolveActivity() needs <queries> on Android 11+, so attempt the start and catch instead.
  private fun startActivity(intent: Intent): Boolean {
    return try {
      activity.startActivity(intent)
      true
    } catch (e: ActivityNotFoundException) {
      false
    } catch (e: SecurityException) {
      false
    }
  }

  private fun status(value: String): JSObject {
    val result = JSObject()
    result.put("status", value)
    return result
  }

  private fun parseSha256(body: String): String {
    val hex = body.trim().split(Regex("\\s+")).firstOrNull()?.lowercase() ?: ""
    if (!hex.matches(SHA256_HEX)) throw IOException("Malformed checksum file")
    return hex
  }

  private fun sha256Of(file: File): String {
    val digest = MessageDigest.getInstance("SHA-256")
    file.inputStream().use { input ->
      val buffer = ByteArray(64 * 1024)
      while (true) {
        val read = input.read(buffer)
        if (read < 0) break
        digest.update(buffer, 0, read)
      }
    }
    return hex(digest.digest())
  }

  private fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }

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

  /** Streams to a `.part` file and only renames it to [target] once the hash matches. */
  private fun download(url: String, target: File, expectedSha256: String, progress: Channel?) {
    val part = File(target.parentFile, "${target.name}.part")
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
        part.outputStream().use { output ->
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
      if (hex(digest.digest()) != expectedSha256) {
        throw IOException("Checksum mismatch, the download was discarded")
      }
      if (!part.renameTo(target)) throw IOException("Could not save the update")
    } finally {
      part.delete()
      conn.disconnect()
    }
  }
}
