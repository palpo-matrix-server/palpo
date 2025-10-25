package csapi_tests

import (
	"github.com/tidwall/gjson"
	"testing"

	"github.com/matrix-org/complement"
	"github.com/matrix-org/complement/b"
	"github.com/matrix-org/complement/client"
	"github.com/matrix-org/complement/helpers"
	"github.com/matrix-org/complement/runtime"
)

// sytest: Left rooms appear in the leave section of sync
func TestSyncLeaveSection(t *testing.T) {
	runtime.SkipIf(t, runtime.Dendrite) // FIXME: https://github.com/matrix-org/dendrite/issues/1323

	deployment := complement.Deploy(t, 1)
	defer deployment.Destroy(t)

	alice := deployment.Register(t, "hs1", helpers.RegistrationOpts{})

	includeLeaveFilter := createFilter(t, alice, map[string]interface{}{
		"room": map[string]interface{}{
			"include_leave": true,
		},
	})

	roomID := alice.MustCreateRoom(t, map[string]interface{}{})

	alice.MustSyncUntil(t, client.SyncReq{}, client.SyncJoinedTo(alice.UserID, roomID))

	s := func(req client.SyncReq) string {
		req.TimeoutMillis = "0"
		_, token := alice.MustSync(t, req)
		return token
	}

	// Get independently-fetched sync tokens for every subsequent test,
	// to isolate interference/side effects otherwise possibly caused by
	// multiple sync calls reusing the same token with different filters or
	// settings.
	fullStateSince := s(client.SyncReq{
		Filter: includeLeaveFilter,
	})
	incrementalSince := s(client.SyncReq{})

	alice.MustLeaveRoom(t, roomID)

	// sytest: Left rooms appear in the leave section of sync
	t.Run("Left rooms appear in the leave section of sync", func(t *testing.T) {
		// SyncLeftFrom does "active" probing of rooms.leave if userID == clientUserID
		alice.MustSyncUntil(t, client.SyncReq{
			Filter: includeLeaveFilter,
		}, client.SyncLeftFrom(alice.UserID, roomID))
	})

	// sytest: Left rooms appear in the leave section of full state sync
	t.Run("Left rooms appear in the leave section of full state sync", func(t *testing.T) {
		alice.MustSyncUntil(t, client.SyncReq{
			Since:     fullStateSince,
			Filter:    includeLeaveFilter,
			FullState: true,
		}, client.SyncLeftFrom(alice.UserID, roomID))
	})

	// sytest: Newly left rooms appear in the leave section of incremental sync
	t.Run("Newly left rooms appear in the leave section of incremental sync", func(t *testing.T) {
		alice.MustSyncUntil(t, client.SyncReq{
			Since: incrementalSince,
		}, client.SyncLeftFrom(alice.UserID, roomID))
	})
}

// sytest: Newly left rooms appear in the leave section of gapped sync
func TestGappedSyncLeaveSection(t *testing.T) {
	runtime.SkipIf(t, runtime.Dendrite) // FIXME: https://github.com/matrix-org/dendrite/issues/1323

	deployment := complement.Deploy(t, 1)
	defer deployment.Destroy(t)

	alice := deployment.Register(t, "hs1", helpers.RegistrationOpts{})

	gappyFilter := createFilter(t, alice, map[string]interface{}{
		"room": map[string]interface{}{
			"timeline": map[string]int64{
				"limit": 1,
			},
			"include_leave": true,
		},
	})

	roomToLeave := alice.MustCreateRoom(t, map[string]interface{}{})
	// j0j0: The original sytest creates an additional room to send events into,
	//  my only suspicion as to why is to trigger a gapped-sync bug,
	//  to see if it "skips" over the leave event.
	roomToSpam := alice.MustCreateRoom(t, map[string]interface{}{})

	_, sinceToken := alice.MustSync(t, client.SyncReq{Filter: gappyFilter, TimeoutMillis: "0"})

	alice.MustLeaveRoom(t, roomToLeave)

	for i := 0; i < 20; i++ {
		alice.SendEventSynced(t, roomToSpam, b.Event{
			Type: "m.room.message",
			Content: map[string]interface{}{
				"msgtype": "m.text",
				"body":    "Mr Anderson...",
			},
		})
	}

	alice.MustSyncUntil(t, client.SyncReq{Filter: gappyFilter, Since: sinceToken}, client.SyncLeftFrom(alice.UserID, roomToLeave))
}

// sytest: Archived rooms only contain history from before the user left
// ... plus later additions
func TestArchivedRoomsHistory(t *testing.T) {
	runtime.SkipIf(t, runtime.Dendrite) // FIXME: https://github.com/matrix-org/dendrite/issues/1323

	deployment := complement.Deploy(t, 1)
	defer deployment.Destroy(t)

	alice := deployment.Register(t, "hs1", helpers.RegistrationOpts{})
	bob := deployment.Register(t, "hs1", helpers.RegistrationOpts{})

	const madeUpTestStateType = "a.madeup.test.state"

	filter := map[string]interface{}{
		"room": map[string]interface{}{
			"timeline": map[string][]string{
				"types": {
					"m.room.message",
					madeUpTestStateType,
				},
			},
			"state": map[string][]string{
				"types": {
					madeUpTestStateType,
				},
			},
			"include_leave": true,
		},
	}

	//aliceFilter := createFilter(t, alice, filter)
	bobFilter := createFilter(t, bob, filter)

	roomID := alice.MustCreateRoom(t, map[string]interface{}{
		"preset": "public_chat",
	})

	bob.MustJoinRoom(t, roomID, nil)
	bob.MustSyncUntil(t, client.SyncReq{}, client.SyncJoinedTo(bob.UserID, roomID))

	_, bobSince := bob.MustSync(t, client.SyncReq{Filter: bobFilter, TimeoutMillis: "0"})

	alice.SendEventSynced(t, roomID, b.Event{
		Type: "m.room.message",
		Content: map[string]interface{}{
			"msgtype": "m.text",
			"body":    "before",
		},
	})

	alice.SendEventSynced(t, roomID, b.Event{
		Type:     madeUpTestStateType,
		StateKey: b.Ptr(""),
		Content: map[string]interface{}{
			"my_key": "before",
		},
	})

	bob.MustLeaveRoom(t, roomID)
	alice.MustSyncUntil(t, client.SyncReq{}, client.SyncLeftFrom(bob.UserID, roomID))

	alice.SendEventSynced(t, roomID, b.Event{
		Type: "m.room.message",
		Content: map[string]interface{}{
			"msgtype": "m.text",
			"body":    "after",
		},
	})

	alice.SendEventSynced(t, roomID, b.Event{
		Type:     madeUpTestStateType,
		StateKey: b.Ptr(""),
		Content: map[string]interface{}{
			"my_key": "after",
		},
	})

	// Test the scenario where the /sync request returns events in the timeline.
	// (Since Alice sent a pair of events before Bob left, this is the normal case.)
	t.Run("timeline has events", func(t *testing.T) {
		exhaustiveResCheck := func(t *testing.T, result gjson.Result, origin string) {
			roomRes := result.Get("rooms.leave." + client.GjsonEscape(roomID))

			stateEvents := roomRes.Get("state.events")
			if stateEvents.Exists() {
				if !stateEvents.IsArray() {
					t.Fatalf("state.events is not an array")
				}
				if len(stateEvents.Array()) != 0 {
					t.Fatalf("expected no state events in %s", origin)
				}
			}

			timelineEvents := roomRes.Get("timeline.events")
			if !timelineEvents.Exists() {
				t.Fatalf("timeline.events does not exist in %s", origin)
			}
			if !timelineEvents.IsArray() {
				t.Fatalf("timeline.events is not an array")
			}
			if len(timelineEvents.Array()) != 2 {
				t.Fatalf("Expected two timeline events in %s", origin)
			}

			timelineEvent := timelineEvents.Array()[0]
			if timelineEvent.Get("content.body").Str != "before" {
				t.Fatalf("Expected only events from before leaving in %s", origin)
			}
		}

		t.Run("initial sync", func(t *testing.T) {
			syncRes, _ := bob.MustSync(t, client.SyncReq{Filter: bobFilter})
			exhaustiveResCheck(t, syncRes, "syncRes")
		})

		t.Run("incremental sync", func(t *testing.T) {
			sinceSyncRes, _ := bob.MustSync(t, client.SyncReq{Filter: bobFilter, Since: bobSince})
			exhaustiveResCheck(t, sinceSyncRes, "sinceSyncRes")
		})
	})

	// Test the scenario where the /sync request returns an *empty* timeline.
	// We arrange for this by setting `limit: 0` on the /sync request.
	// This is a regression test for a bug that was fixed in https://github.com/element-hq/synapse/pull/16932.
	t.Run("timeline is empty", func(t *testing.T) {
		check := func(t *testing.T, result gjson.Result) {
			roomRes := result.Get("rooms.leave." + client.GjsonEscape(roomID))
			t.Logf("Sync result: %s", roomRes)

			// Double-check that the timeline is, indeed, empty.
			timelineEvents := roomRes.Get("timeline.events")
			if !timelineEvents.IsArray() {
				t.Fatalf("timeline.events is not an array")
			}
			if len(timelineEvents.Array()) != 0 {
				t.Fatalf("Expected no timeline events")
			}

			// `state` should contain the leave event and the madeup test state from *before* the leave.
			stateEvents := roomRes.Get("state.events")
			if !stateEvents.IsArray() {
				t.Fatalf("state.events is not an array")
			}
			foundLeave := false
			for _, event := range stateEvents.Array() {
				if event.Get("type").Str == madeUpTestStateType {
					if event.Get("content.my_key").Str != "before" {
						t.Errorf("state should not include state from after the user left.")
					}
				}

				if event.Get("type").Str == "m.room.member" && event.Get("state_key").Str == bob.UserID {
					membership := event.Get("content.membership").Str
					if membership == "leave" {
						foundLeave = true
					} else {
						t.Errorf("Expected Bob's leave event; got %s", membership)
					}
				}
			}

			if !foundLeave {
				t.Errorf("Didn't see Bob's leave event")
			}
		}

		// Construct a timeline filter which will cause the timeline to be empty
		filter := `{ "room": {
              "timeline": { "limit": 0 },
              "include_leave": true
        }}`
		t.Run("initial sync", func(t *testing.T) {
			syncRes, _ := bob.MustSync(t, client.SyncReq{Filter: filter})
			check(t, syncRes)
		})

		t.Run("incremental sync", func(t *testing.T) {
			t.Skip("Synapse doesn't return the room at all!")
			sinceSyncRes, _ := bob.MustSync(t, client.SyncReq{Filter: filter, Since: bobSince})
			check(t, sinceSyncRes)
		})
	})
}

// This tests if rooms in the leave section get removed after a sync.
//
// sytest: Previously left rooms don't appear in the leave section of sync
func TestOlderLeftRoomsNotInLeaveSection(t *testing.T) {
	runtime.SkipIf(t, runtime.Dendrite) // FIXME: https://github.com/matrix-org/dendrite/issues/1323

	deployment := complement.Deploy(t, 1)
	defer deployment.Destroy(t)

	alice := deployment.Register(t, "hs1", helpers.RegistrationOpts{})
	bob := deployment.Register(t, "hs1", helpers.RegistrationOpts{})

	aliceFilter := createFilter(t, alice, map[string]interface{}{
		"room": map[string]interface{}{
			"timeline": map[string]int64{
				"limit": 1,
			},
			"include_leave": true,
		},
	})

	roomToLeave := alice.MustCreateRoom(t, map[string]interface{}{
		"preset": "public_chat",
	})
	roomToSpam := alice.MustCreateRoom(t, map[string]interface{}{
		"preset": "public_chat",
	})

	bob.MustJoinRoom(t, roomToLeave, nil)
	bob.MustJoinRoom(t, roomToSpam, nil)
	bob.MustSyncUntil(
		t,
		client.SyncReq{},
		client.SyncJoinedTo(bob.UserID, roomToLeave),
		client.SyncJoinedTo(bob.UserID, roomToSpam),
	)

	_, aliceSince := alice.MustSync(t, client.SyncReq{Filter: aliceFilter, TimeoutMillis: "0"})

	alice.MustLeaveRoom(t, roomToLeave)

	aliceSince = alice.MustSyncUntil(t, client.SyncReq{Filter: aliceFilter, Since: aliceSince}, client.SyncLeftFrom(alice.UserID, roomToLeave))

	bob.SendEventSynced(t, roomToLeave, b.Event{
		Type:     "m.room.member",
		StateKey: b.Ptr(bob.UserID),
		Content: map[string]interface{}{
			"membership": "join",
			"filler":     "Welcome back, my friends, to the show that never ends...",
		},
	})

	// Pad out the timeline with filler messages to create a "gap" between
	// this sync and the next.
	for i := 0; i < 20; i++ {
		bob.SendEventSynced(t, roomToSpam, b.Event{
			Type: "m.room.message",
			Content: map[string]interface{}{
				"msgtype": "m.text",
				"body":    "Mr Anderson...",
			},
		})
	}

	syncRes, _ := alice.MustSync(t, client.SyncReq{Filter: aliceFilter, Since: aliceSince})

	roomLeaveRes := syncRes.Get("rooms.leave")
	if roomLeaveRes.Exists() {
		if !roomLeaveRes.IsObject() {
			t.Fatalf("rooms.leave is not an object in syncRes")
		}

		if len(roomLeaveRes.Map()) != 0 {
			t.Fatalf("Expected no rooms in 'leave' state")
		}
	}
}

// sytest: We should see our own leave event, even if history_visibility is
//
//	restricted (SYN-662)
func TestLeaveEventVisibility(t *testing.T) {
	runtime.SkipIf(t, runtime.Dendrite) // FIXME: https://github.com/matrix-org/dendrite/issues/1323

	// Note: this deviates from the sytest by introducing a second user,
	//  this user is only meant to keep the room alive,
	//  as a room with no users may be purged by the server,
	//  creating side effects that this test is not looking for.
	deployment := complement.Deploy(t, 1)
	defer deployment.Destroy(t)

	alice := deployment.Register(t, "hs1", helpers.RegistrationOpts{})
	bob := deployment.Register(t, "hs1", helpers.RegistrationOpts{})

	aliceFilter := createFilter(t, alice, map[string]interface{}{
		"room": map[string]interface{}{
			"include_leave": true,
		},
	})

	roomID := alice.MustCreateRoom(t, map[string]interface{}{
		"initial_state": []map[string]interface{}{
			{
				"content": map[string]interface{}{
					"history_visibility": "joined",
				},
				"type":      "m.room.history_visibility",
				"state_key": "",
			},
		},
		"preset": "public_chat",
	})
	bob.MustJoinRoom(t, roomID, nil)

	aliceSince := alice.MustSyncUntil(
		t,
		client.SyncReq{Filter: aliceFilter},
		client.SyncJoinedTo(alice.UserID, roomID),
		client.SyncJoinedTo(bob.UserID, roomID),
	)

	alice.MustLeaveRoom(t, roomID)
	bob.MustSyncUntil(t, client.SyncReq{}, client.SyncLeftFrom(alice.UserID, roomID))

	syncRes, _ := alice.MustSync(t, client.SyncReq{Filter: aliceFilter, Since: aliceSince, TimeoutMillis: "0"})

	roomRes := syncRes.Get("rooms.leave." + client.GjsonEscape(roomID))

	if !roomRes.Exists() {
		t.Fatalf("could not find %s in rooms.leave", roomID)
	}

	stateResult := roomRes.Get("state.events")

	if stateResult.Exists() {
		if !stateResult.IsArray() {
			t.Fatalf("state.events was not an array")
		} else {
			if len(stateResult.Array()) != 0 {
				t.Fatalf("Expected no state events")
			}
		}
	}

	timelineRes := roomRes.Get("timeline.events")

	if !timelineRes.Exists() {
		t.Fatalf("could not find timeline.events")
	} else if !timelineRes.IsArray() {
		t.Fatalf("timeline.events was not an array")
	} else if len(timelineRes.Array()) != 1 {
		t.Fatalf("expected exactly 1 item in timeline")
	}

	event := timelineRes.Array()[0]

	if event.Get("type").Str != "m.room.member" {
		t.Fatalf("expected timeline event was not m.room.member")
	} else if event.Get("sender").Str != alice.UserID {
		t.Fatalf("sender of membership event was not alice")
	} else if event.Get("content.membership").Str != "leave" {
		t.Fatalf("membership event was not leave")
	}
}

// sytest: We should see our own leave event when rejecting an invite,
//
//	even if history_visibility is restricted (riot-web/3462)
func TestLeaveEventInviteRejection(t *testing.T) {
	runtime.SkipIf(t, runtime.Dendrite) // FIXME: https://github.com/matrix-org/dendrite/issues/1323

	deployment := complement.Deploy(t, 1)
	defer deployment.Destroy(t)

	alice := deployment.Register(t, "hs1", helpers.RegistrationOpts{})
	bob := deployment.Register(t, "hs1", helpers.RegistrationOpts{})

	aliceFilter := createFilter(t, alice, map[string]interface{}{
		"room": map[string]interface{}{
			"include_leave": true,
		},
	})

	roomID := bob.MustCreateRoom(t, map[string]interface{}{
		"initial_state": []map[string]interface{}{
			{
				"content": map[string]interface{}{
					"history_visibility": "joined",
				},
				"type":      "m.room.history_visibility",
				"state_key": "",
			},
		},
	})

	_, aliceSince := alice.MustSync(t, client.SyncReq{TimeoutMillis: "0", Filter: aliceFilter})

	bob.MustInviteRoom(t, roomID, alice.UserID)

	aliceSince = alice.MustSyncUntil(
		t,
		client.SyncReq{Filter: aliceFilter, Since: aliceSince},
		client.SyncInvitedTo(alice.UserID, roomID),
	)

	alice.MustLeaveRoom(t, roomID)

	aliceSince = alice.MustSyncUntil(
		t,
		client.SyncReq{Filter: aliceFilter, Since: aliceSince},
		client.SyncLeftFrom(alice.UserID, roomID),
	)
}
