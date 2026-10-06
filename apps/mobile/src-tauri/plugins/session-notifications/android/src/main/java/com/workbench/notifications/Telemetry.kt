package com.workbench.notifications

import android.content.Context
import android.content.pm.ApplicationInfo
import io.sentry.Breadcrumb
import io.sentry.Sentry
import io.sentry.SentryLevel
import io.sentry.android.core.SentryAndroid

/**
 * Sentry for the native side (crashes, ANRs, the notification service's own
 * diagnostics). Release builds only; every call is a no-op until [init] runs.
 * Never record tokens, URLs or session titles.
 */
object Telemetry {
  // The desktop's public ingest DSN (write-only key); the release tells the apps apart.
  private const val DSN = "https://708b68ab317b3d17816c9f8135337d11@sentry.starkeydigital.com/20"
  private val reported = mutableSetOf<String>()

  fun init(context: Context) {
    if (Sentry.isEnabled() || context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) return
    val version = context.packageManager.getPackageInfo(context.packageName, 0).versionName
    SentryAndroid.init(context) { options ->
      options.dsn = DSN
      options.release = "workbench-mobile@$version"
      options.environment = "production"
      options.isEnableAutoSessionTracking = false
      options.isSendDefaultPii = false
    }
  }

  fun breadcrumb(message: String) {
    Sentry.addBreadcrumb(Breadcrumb.info(message).apply { category = "notifications" })
  }

  /** Report `key` at most once per process, so repeated request failures can't flood Sentry. */
  fun once(key: String, message: String, extras: Map<String, Any?> = emptyMap()) {
    synchronized(reported) { if (!reported.add(key)) return }
    Sentry.captureMessage(message, SentryLevel.WARNING) { scope ->
      extras.forEach { (k, v) -> scope.setExtra(k, v.toString()) }
    }
  }

  /** The class and stack only: messages can quote the server's reply (session titles). */
  fun exceptionOnce(e: Throwable) {
    synchronized(reported) { if (!reported.add(e.javaClass.name)) return }
    Sentry.captureException(RuntimeException(e.javaClass.name).apply { stackTrace = e.stackTrace })
  }
}
