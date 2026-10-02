package com.workbench.notifications

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.PermissionState
import app.tauri.plugin.Plugin
import java.net.URI

@InvokeArg
class StartArgs {
  lateinit var url: String
  lateinit var token: String
  lateinit var machineId: String
  lateinit var name: String
  lateinit var sessionId: String
}

@TauriPlugin(permissions = [Permission(strings = [Manifest.permission.POST_NOTIFICATIONS], alias = "notifications")])
class SessionNotificationsPlugin(private val activity: Activity) : Plugin(activity) {
  private var pending: String? = null

  @Command
  fun start(invoke: Invoke) {
    if (Build.VERSION.SDK_INT >= 33 && getPermissionState("notifications") != PermissionState.GRANTED) {
      requestPermissionForAlias("notifications", invoke, "permissionResult")
    } else startAllowed(invoke)
  }

  @PermissionCallback
  fun permissionResult(invoke: Invoke) {
    if (getPermissionState("notifications") != PermissionState.GRANTED) invoke.reject("Notifications are disabled. Allow them in Android settings.")
    else startAllowed(invoke)
  }

  private fun startAllowed(invoke: Invoke) {
    try {
      if (!NotificationManagerCompat.from(activity).areNotificationsEnabled()) {
        invoke.reject("Notifications are disabled. Allow them in Android settings.")
        return
      }
      val args = invoke.parseArgs(StartArgs::class.java)
      val uri = URI(args.url)
      require(uri.scheme in listOf("http", "https") && uri.host != null && uri.userInfo == null && uri.query == null && uri.fragment == null && (uri.path.isNullOrEmpty() || uri.path == "/")) { "Invalid server address" }
      require(args.token.length >= 32 && !args.token.contains('\n') && !args.token.contains('\r')) { "Invalid server token" }
      val service = Intent(activity, SessionNotificationService::class.java)
        .putExtra("url", args.url.trimEnd('/')).putExtra("token", args.token)
        .putExtra("machineId", args.machineId).putExtra("name", args.name).putExtra("sessionId", args.sessionId)
      ContextCompat.startForegroundService(activity, service)
      invoke.resolve()
    } catch (e: Exception) { invoke.reject(e.message ?: "Couldn't start notifications") }
  }

  @Command
  fun stop(invoke: Invoke) {
    activity.stopService(Intent(activity, SessionNotificationService::class.java))
    invoke.resolve()
  }

  @Command
  fun takeOpenSession(invoke: Invoke) {
    val raw = pending ?: activity.intent.getStringExtra("workbench.session")
    pending = null
    activity.intent.removeExtra("workbench.session")
    val result = JSObject()
    result.put("session", raw)
    invoke.resolve(result)
  }

  override fun onNewIntent(intent: Intent) {
    pending = intent.getStringExtra("workbench.session")
    if (pending != null) trigger("open", JSObject())
  }
  override fun onResume() { SessionNotificationService.visible = true }
  override fun onPause() { SessionNotificationService.visible = false }
}
