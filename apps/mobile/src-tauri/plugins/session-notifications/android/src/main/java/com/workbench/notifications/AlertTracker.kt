package com.workbench.notifications

import org.json.JSONArray
import org.json.JSONObject

data class SessionState(val busy: Boolean, val waiting: String?, val updated: Long)
data class SessionAlert(val session: JSONObject, val message: String)

/** Seed completions on the first list, deduplicate approvals, and follow /clear aliases. */
class AlertTracker {
  private var states = mapOf<String, SessionState>()
  fun update(list: JSONArray): List<SessionAlert> {
    val next = mutableMapOf<String, SessionState>()
    val alerts = mutableListOf<SessionAlert>()
    for (i in 0 until list.length()) {
      val s = list.getJSONObject(i)
      val id = s.getString("sessionId")
      val aliases = s.optJSONArray("previousIds") ?: JSONArray()
      val old = states[id] ?: (0 until aliases.length()).mapNotNull { states[aliases.getString(it)] }.firstOrNull()
      val waiting = s.optJSONObject("waiting")?.optString("id")
      val busy = s.optBoolean("busy")
      val updated = s.optLong("updatedAt")
      if (waiting != null && waiting != old?.waiting) alerts.add(SessionAlert(s, "Approval or answer needed"))
      else if (old?.busy == true && !busy && waiting == null && !s.optBoolean("exited") && updated >= old.updated) alerts.add(SessionAlert(s, "Turn complete"))
      next[id] = SessionState(busy, waiting, updated)
    }
    states = next
    return alerts
  }
}
