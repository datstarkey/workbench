package com.workbench.notifications

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
import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

/** Native polling survives a suspended WebView. The bearer token stays in memory only. */
class SessionNotificationService : Service() {
  companion object { @Volatile var visible = true }
  private val executor = Executors.newSingleThreadScheduledExecutor()
  private val main = Handler(Looper.getMainLooper())
  private var task: ScheduledFuture<*>? = null
  private var generation = 0
  private var tracker = AlertTracker()
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
    tracker = AlertTracker()
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
    task = executor.scheduleWithFixedDelay({
      noteStall()
      try {
        val connection = URL("$url/agent").openConnection() as HttpURLConnection
        connection.instanceFollowRedirects = false
        connection.connectTimeout = 8000
        connection.readTimeout = 8000
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
          val list = JSONArray(String(body, Charsets.UTF_8))
          main.post {
            if (generation != current) return@post
            for (alert in tracker.update(list)) {
              if (visible) { Telemetry.breadcrumb("alert skipped: app visible"); continue }
              noteBlocked()
              val s = alert.session
              val sessionId = s.getString("sessionId")
              val payload = JSONObject().put("machineId", machineId).put("chat", s).toString()
              val notificationId = 2 + (sessionId.hashCode() and 0x3fffffff)
              val alertNotification = NotificationCompat.Builder(this, "sessions")
                .setSmallIcon(android.R.drawable.ic_dialog_info).setContentTitle(alert.message)
                .setContentText(if (s.isNull("title")) "Workbench chat" else s.getString("title")).setSubText(name)
                .setVisibility(NotificationCompat.VISIBILITY_PRIVATE).setAutoCancel(true)
                .setContentIntent(open(payload, notificationId)).build()
              manager.notify(notificationId, alertNotification)
              Telemetry.breadcrumb("alert posted: ${alert.message}")
            }
          }
        } finally { connection.disconnect() }
      } catch (e: IOException) {
        // Retry when the private network is reachable again.
        Telemetry.breadcrumb("poll failed: ${e.javaClass.simpleName}")
      } catch (e: Exception) {
        Telemetry.exceptionOnce(e)
      }
    }, 0, 10, TimeUnit.SECONDS)
    return START_NOT_STICKY
  }

  /** A gap far past the 10s interval means polling was frozen (the CPU slept, or Doze). */
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
    executor.shutdownNow()
    manager.cancel(monitorId)
    super.onDestroy()
  }
}
