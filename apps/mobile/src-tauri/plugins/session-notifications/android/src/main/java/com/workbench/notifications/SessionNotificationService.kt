package com.workbench.notifications

import android.app.ActivityManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import org.json.JSONObject
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

/** Native notification delivery survives a suspended WebView. Tokens stay in memory only. */
class SessionNotificationService : Service() {
  private val executor = Executors.newSingleThreadScheduledExecutor()
  private val main = Handler(Looper.getMainLooper())
  private var task: ScheduledFuture<*>? = null
  @Volatile private var generation = 0
  @Volatile private var activeConnection: HttpURLConnection? = null
  private val manager get() = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
  private val monitorId = 1
  /** Wall clock of the last poll: the executor's clock stops while the phone sleeps. */
  private var lastPollAt = 0L

  override fun onBind(intent: Intent?): IBinder? = null
  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent?.action == "stop") { stopSelf(); return START_NOT_STICKY }
    val url = intent?.getStringExtra("url") ?: run { stopSelf(); return START_NOT_STICKY }
    val token = intent.getStringExtra("token") ?: run { stopSelf(); return START_NOT_STICKY }
    val machineId = intent.getStringExtra("machineId") ?: run { stopSelf(); return START_NOT_STICKY }
    val name = intent.getStringExtra("name") ?: "Workbench"
    generation++
    val current = generation
    task?.cancel(true)
    activeConnection?.disconnect()
    // Switching machines clears old alerts so nothing opens on the wrong connection.
    manager.cancelAll()
    if (Build.VERSION.SDK_INT >= 26) {
      manager.createNotificationChannel(NotificationChannel("monitor", "Session monitoring", NotificationManager.IMPORTANCE_LOW))
      manager.createNotificationChannel(NotificationChannel("sessions", "Approvals and completed turns", NotificationManager.IMPORTANCE_DEFAULT))
    }
    val stop = PendingIntent.getService(this, 0, Intent(this, javaClass).setAction("stop"), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    val notification = NotificationCompat.Builder(this, "monitor")
      .setSmallIcon(android.R.drawable.ic_dialog_info).setContentTitle("Workbench · $name")
      .setContentText("Watching sessions for approvals and completed turns").setOngoing(true)
      .setContentIntent(open(null, 0)).addAction(0, "Stop", stop).build()
    if (Build.VERSION.SDK_INT >= 34) startForeground(monitorId, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_REMOTE_MESSAGING)
    else startForeground(monitorId, notification)
    lastPollAt = 0L
    Telemetry.breadcrumb("monitoring started")
    val currentFeed = NotificationFeed()
    task = executor.scheduleWithFixedDelay({
      if (generation != current) return@scheduleWithFixedDelay
      noteStall()
      try {
        val cursor = currentFeed.cursor
        val route = "/agent/attention" +
          (cursor?.let { "?cursor=" + URLEncoder.encode(it, "UTF-8") } ?: "")
        val connection = URL("$url$route").openConnection() as HttpURLConnection
        activeConnection = connection
        if (generation != current) {
          connection.disconnect()
          if (activeConnection === connection) activeConnection = null
          return@scheduleWithFixedDelay
        }
        connection.instanceFollowRedirects = false
        connection.connectTimeout = 8000
        connection.readTimeout = 25_000
        connection.setRequestProperty("Authorization", "Bearer $token")
        try {
          val code = connection.responseCode
          if (code == 401 || code == 403) { main.post { if (generation == current) stopSelf() }; return@scheduleWithFixedDelay }
          if (code != 200) {
            Telemetry.once("http-$code", "Session poll got HTTP $code")
            return@scheduleWithFixedDelay
          }
          val body = connection.inputStream.use { it.readBytes() }
          if (body.size > 2 * 1024 * 1024) return@scheduleWithFixedDelay
          val text = String(body, Charsets.UTF_8)
          main.post {
            if (generation != current) return@post
            try {
              val events = currentFeed.accept(JSONObject(text))
              for (event in events) notifySession(event, machineId, name)
            } catch (e: Exception) { Telemetry.exceptionOnce(e) }
          }
        } finally { connection.disconnect(); if (activeConnection === connection) activeConnection = null }
      } catch (e: IOException) {
        // Retry when the private network is reachable again.
        Telemetry.breadcrumb("poll failed: ${e.javaClass.simpleName}")
      } catch (e: Exception) {
        Telemetry.exceptionOnce(e)
      }
    }, 0, 1, TimeUnit.SECONDS)
    return START_NOT_STICKY
  }

  /** Post, replace or clear the same event the desktop consumed. */
  private fun notifySession(event: NotificationEvent, machineId: String, name: String) {
    val s = event.session
    val sessionId = s.getString("sessionId")
    val notificationId = 2 + (sessionId.hashCode() and 0x3fffffff)
    // /clear changes the id; its old alert must not remain behind.
    val aliases = s.optJSONArray("previousIds")
    if (aliases != null) for (i in 0 until aliases.length()) {
      manager.cancel(2 + (aliases.getString(i).hashCode() and 0x3fffffff))
    }
    if (event.message == null) { manager.cancel(notificationId); return }
    if (appVisible()) { Telemetry.breadcrumb("alert skipped: app visible"); return }
    noteBlocked()
    val payload = JSONObject().put("machineId", machineId).put("chat", s).toString()
    val notification = NotificationCompat.Builder(this, "sessions")
      .setSmallIcon(android.R.drawable.ic_dialog_info).setContentTitle(event.message)
      .setContentText(if (s.isNull("title")) "Workbench chat" else s.getString("title")).setSubText(name)
      .setVisibility(NotificationCompat.VISIBILITY_PRIVATE).setAutoCancel(true)
      .setContentIntent(open(payload, notificationId)).build()
    manager.notify(notificationId, notification)
    Telemetry.breadcrumb("alert posted: ${event.message}")
  }

  /** Asked at post time: Tauri never registers the observer that forwards onPause to plugins. */
  private fun appVisible(): Boolean {
    val state = ActivityManager.RunningAppProcessInfo()
    ActivityManager.getMyMemoryState(state)
    return state.importance == ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND
  }

  /** A gap past long polling's timeout means the CPU slept or the network stalled. */
  private fun noteStall() {
    val now = System.currentTimeMillis()
    val gap = now - lastPollAt
    if (lastPollAt != 0L && gap > 60_000) {
      val power = getSystemService(POWER_SERVICE) as PowerManager
      Telemetry.once("stall", "Session polling stalled", mapOf(
        "gapSeconds" to gap / 1000,
        "deviceIdle" to (Build.VERSION.SDK_INT >= 23 && power.isDeviceIdleMode),
        "interactive" to power.isInteractive,
        "ignoringBatteryOptimizations" to (Build.VERSION.SDK_INT >= 23 && power.isIgnoringBatteryOptimizations(packageName)),
      ))
    }
    lastPollAt = now
  }

  /** The person (or Android) turned alerts off: they would post into nothing. */
  private fun noteBlocked() {
    val channel = if (Build.VERSION.SDK_INT >= 26) manager.getNotificationChannel("sessions") else null
    val blocked = !manager.areNotificationsEnabled() || channel?.importance == NotificationManager.IMPORTANCE_NONE
    if (blocked) Telemetry.once("blocked", "Session alerts are blocked in Android settings")
  }

  private fun open(payload: String?, id: Int): PendingIntent {
    val intent = packageManager.getLaunchIntentForPackage(packageName)!!.apply {
      addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
      if (payload != null) putExtra("workbench.session", payload)
    }
    return PendingIntent.getActivity(this, id, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
  }

  override fun onDestroy() {
    generation++
    task?.cancel(true)
    activeConnection?.disconnect()
    executor.shutdownNow()
    manager.cancel(monitorId)
    super.onDestroy()
  }
}
