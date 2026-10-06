package com.workbench.notifications

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class NotificationFeedTest {
  private fun event(kind: String, id: String = "s", agent: String = "codex") = JSONObject()
    .put("kind", kind).put("sessionId", id).put("agent", agent)
    .put("projectPath", "/project").put("worktreePath", "/worktree")
    .put("claudeAccountId", "account").put("previousIds", JSONArray().put("old"))
  private fun batch(cursor: String, vararg events: JSONObject) = JSONObject()
    .put("cursor", cursor).put("events", JSONArray(events.toList()))

  @Test fun theServerCursorSeedsAndDeduplicatesRetriedBatches() {
    val feed = NotificationFeed()
    assertTrue(feed.accept(batch("server:0")).isEmpty())
    val turn = batch("server:1", event("turnEnded"))
    assertEquals("Turn complete", feed.accept(turn).single().message)
    assertTrue(feed.accept(turn).isEmpty())
    assertEquals("server:1", feed.cursor)
  }

  @Test fun bothAgentsUseTheServersExactApprovalCompletionAndResolutionEvents() {
    val feed = NotificationFeed()
    val events = feed.accept(batch("server:3", event("waiting", agent = "claude"), event("resolved", agent = "claude"), event("turnEnded")))
    assertEquals(listOf("Approval or answer needed", null, "Turn complete"), events.map { it.message })
    assertEquals(listOf("claude", "claude", "codex"), events.map { it.session.getString("agent") })
    assertEquals("account", events[0].session.getString("claudeAccountId"))
    assertEquals("old", events[0].session.getJSONArray("previousIds").getString(0))
  }

  @Test fun restartingTheServerSeedsANewEpochWithoutInventingAlerts() {
    val feed = NotificationFeed()
    feed.accept(batch("old:50"))
    assertTrue(feed.accept(batch("new:0")).isEmpty())
    assertEquals("Turn complete", feed.accept(batch("new:1", event("turnEnded"))).single().message)
  }

  @Test fun malformedBatchesDoNotAdvanceTheCursor() {
    val feed = NotificationFeed()
    feed.accept(batch("server:0"))
    try { feed.accept(JSONObject().put("cursor", "server:1")); fail("invalid batch") }
    catch (_: org.json.JSONException) {}
    assertEquals("server:0", feed.cursor)
    assertEquals(1, feed.accept(batch("server:1", event("waiting"))).size)
  }
}
