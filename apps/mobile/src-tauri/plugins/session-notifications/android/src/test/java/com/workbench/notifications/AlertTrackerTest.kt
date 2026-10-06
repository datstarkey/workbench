package com.workbench.notifications

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class AlertTrackerTest {
  private fun summary(id: String = "s", busy: Boolean = false, waiting: String? = null, aliases: List<String> = emptyList(), exited: Boolean = false, turnEnded: Long? = null): JSONObject {
    val s = JSONObject().put("sessionId", id).put("busy", busy).put("updatedAt", 1).put("exited", exited).put("previousIds", JSONArray(aliases))
    if (waiting != null) s.put("waiting", JSONObject().put("id", waiting))
    if (turnEnded != null) s.put("turnEndedAt", turnEnded)
    return s
  }
  private fun session(id: String = "s", busy: Boolean = false, waiting: String? = null, aliases: List<String> = emptyList(), exited: Boolean = false, turnEnded: Long? = null) =
    JSONArray().put(summary(id, busy, waiting, aliases, exited, turnEnded))

  @Test fun theFirstListOnlySeeds() {
    val tracker = AlertTracker()
    assertTrue(tracker.update(session(waiting = "a", turnEnded = 5)).isEmpty())
    assertTrue(tracker.update(session(waiting = "a", turnEnded = 5)).isEmpty())
    assertEquals("Approval or answer needed", tracker.update(session(waiting = "b", turnEnded = 5))[0].message)
  }
  @Test fun completionFromAnObservedBusyTurnOnOlderServers() {
    val tracker = AlertTracker()
    assertTrue(tracker.update(session()).isEmpty())
    assertTrue(tracker.update(session(busy = true)).isEmpty())
    assertEquals("Turn complete", tracker.update(session())[0].message)
    assertTrue(tracker.update(session()).isEmpty())
  }
  @Test fun aTurnBetweenTwoPollsStillCompletes() {
    val tracker = AlertTracker()
    tracker.update(session(turnEnded = 5))
    assertTrue(tracker.update(session(turnEnded = 5)).isEmpty())
    assertEquals("Turn complete", tracker.update(session(turnEnded = 9))[0].message)
    assertTrue(tracker.update(session(turnEnded = 9)).isEmpty())
  }
  @Test fun everySessionIsWatchedAndNewOnesSeedSilently() {
    val tracker = AlertTracker()
    tracker.update(session(id = "a"))
    val both = JSONArray().put(summary(id = "a", turnEnded = 3)).put(summary(id = "b", turnEnded = 3))
    assertEquals(listOf("a"), tracker.update(both).map { it.session.getString("sessionId") })
    val approval = JSONArray().put(summary(id = "a", turnEnded = 3)).put(summary(id = "b", turnEnded = 3)).put(summary(id = "c", waiting = "x"))
    assertEquals(listOf("c"), tracker.update(approval).map { it.session.getString("sessionId") })
  }
  @Test fun approvalsAreDeduplicatedAndFollowClearAliases() {
    val tracker = AlertTracker()
    tracker.update(JSONArray())
    assertEquals(1, tracker.update(session(waiting = "a")).size)
    assertTrue(tracker.update(session(waiting = "a")).isEmpty())
    assertTrue(tracker.update(session(id = "new", waiting = "a", aliases = listOf("s"))).isEmpty())
    assertEquals(1, tracker.update(session(id = "new", waiting = "b")).size)
  }
  @Test fun exitsDoNotPretendToBeSuccessfulCompletions() {
    val tracker = AlertTracker()
    tracker.update(session(busy = true))
    assertTrue(tracker.update(session(exited = true, turnEnded = 4)).isEmpty())
  }
  @Test fun aNewCodexSessionCanFinishItsFirstTurnBetweenPolls() {
    val tracker = AlertTracker()
    tracker.update(JSONArray(), serverTime = 100_000)
    val codex = JSONArray().put(summary(id = "codex", turnEnded = 104_000).put("agent", "codex"))
    assertEquals("Turn complete", tracker.update(codex, serverTime = 110_000).single().message)
    assertTrue(tracker.update(codex, serverTime = 120_000).isEmpty())
  }
  @Test fun oldNewlyDiscoveredCompletionsAndTheFirstPollRemainSilent() {
    val tracker = AlertTracker()
    assertTrue(tracker.update(session(turnEnded = 99_000), serverTime = 100_000).isEmpty())
    assertTrue(tracker.update(session(id = "old", turnEnded = 90_000), serverTime = 110_000).isEmpty())
    // A missing Date never falls back to the phone's potentially different clock.
    tracker.update(JSONArray())
    assertTrue(tracker.update(session(id = "unknown", turnEnded = 120_000)).isEmpty())
  }
  @Test fun bothAgentsAreWatchedAndCodexApprovalsAreDeduplicated() {
    val tracker = AlertTracker()
    val claude = summary(id = "claude").put("agent", "claude")
    val codex = summary(id = "codex").put("agent", "codex")
    tracker.update(JSONArray().put(claude).put(codex))
    codex.put("waiting", JSONObject().put("id", "approval-0"))
    val both = JSONArray().put(claude).put(codex)
    assertEquals("codex", tracker.update(both).single().session.getString("sessionId"))
    assertTrue(tracker.update(both).isEmpty())
    codex.remove("waiting")
    codex.put("turnEndedAt", 5)
    assertEquals("Turn complete", tracker.update(both).single().message)
  }
}
