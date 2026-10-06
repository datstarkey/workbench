package com.workbench.notifications

import org.json.JSONArray
import org.json.JSONObject

data class SessionState(val busy: Boolean, val waiting: String?, val turnEnded: Long)
data class SessionAlert(val session: JSONObject, val message: String)

/**
 * Seed every session on the first list, deduplicate approvals, and follow /clear aliases.
 * A completion is a newer `turnEndedAt` (catches turns shorter than the poll interval) or,
 * from servers that don't send it, an observed busy-to-idle change.
 * HTTP Date anchors newly discovered completions without relying on the phone's clock.
 */
class AlertTracker {
  private var states: Map<String, SessionState>? = null
  private var sampledAt: Long? = null
  fun update(list: JSONArray, serverTime: Long? = null): List<SessionAlert> {
    val previous = states
    val previousSample = sampledAt
    val next = mutableMapOf<String, SessionState>()
    val alerts = mutableListOf<SessionAlert>()
    for (i in 0 until list.length()) {
      val s = list.getJSONObject(i)
      val id = s.getString("sessionId")
      val aliases = s.optJSONArray("previousIds") ?: JSONArray()
      val old = previous?.let { p -> p[id] ?: (0 until aliases.length()).mapNotNull { p[aliases.getString(it)] }.firstOrNull() }
      val waiting = s.optJSONObject("waiting")?.optString("id")
      val busy = s.optBoolean("busy")
      val turnEnded = s.optLong("turnEndedAt", 0L)
      next[id] = SessionState(busy, waiting, turnEnded)
      if (previous == null) continue
      if (waiting != null && waiting != old?.waiting) alerts.add(SessionAlert(s, "Approval or answer needed"))
      else if (!busy && waiting == null && !s.optBoolean("exited")) {
        // A first Codex turn can finish before its session is ever listed. Seed old
        // sessions silently, but alert if it completed since the preceding sample.
        val ended = if (old != null) turnEnded > old.turnEnded || old.busy
          else turnEnded > 0L && previousSample != null && turnEnded >= previousSample
        if (ended) alerts.add(SessionAlert(s, "Turn complete"))
      }
    }
    states = next
    sampledAt = serverTime
    return alerts
  }
}
