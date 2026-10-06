package com.workbench.notifications

import org.json.JSONObject

data class NotificationEvent(val session: JSONObject, val message: String?)

/** The server already decided what deserves attention. Only translate its events. */
class NotificationFeed {
  @Volatile var cursor: String? = null
    private set

  fun accept(batch: JSONObject): List<NotificationEvent> {
    val next = batch.getString("cursor")
    if (next == cursor) return emptyList()
    val events = batch.getJSONArray("events")
    val result = mutableListOf<NotificationEvent>()
    for (i in 0 until events.length()) {
      val session = events.getJSONObject(i)
      session.getString("sessionId")
      when (session.getString("kind")) {
        "waiting" -> result.add(NotificationEvent(session, "Approval or answer needed"))
        "turnEnded" -> result.add(NotificationEvent(session, "Turn complete"))
        "resolved" -> result.add(NotificationEvent(session, null))
      }
    }
    // Advance only after a whole valid batch, so a failed parse can be retried.
    cursor = next
    return result
  }
}
