package com.workbench.notifications

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class AlertTrackerTest {
  private fun session(id: String = "s", busy: Boolean = false, waiting: String? = null, aliases: List<String> = emptyList(), exited: Boolean = false): JSONArray {
    val s = JSONObject().put("sessionId", id).put("busy", busy).put("updatedAt", 1).put("exited", exited).put("previousIds", JSONArray(aliases))
    if (waiting != null) s.put("waiting", JSONObject().put("id", waiting))
    return JSONArray().put(s)
  }
  @Test fun completionRequiresAnObservedBusyTurn() {
    val tracker = AlertTracker()
    assertTrue(tracker.update(session()).isEmpty())
    assertTrue(tracker.update(session(busy = true)).isEmpty())
    assertEquals("Turn complete", tracker.update(session())[0].message)
    assertTrue(tracker.update(session()).isEmpty())
  }
  @Test fun approvalsAreDeduplicatedAndFollowClearAliases() {
    val tracker = AlertTracker()
    assertEquals(1, tracker.update(session(waiting = "a")).size)
    assertTrue(tracker.update(session(waiting = "a")).isEmpty())
    assertTrue(tracker.update(session(id = "new", waiting = "a", aliases = listOf("s"))).isEmpty())
    assertEquals(1, tracker.update(session(id = "new", waiting = "b")).size)
  }
  @Test fun exitsDoNotPretendToBeSuccessfulCompletions() {
    val tracker = AlertTracker()
    tracker.update(session(busy = true))
    assertTrue(tracker.update(session(exited = true)).isEmpty())
  }
}
