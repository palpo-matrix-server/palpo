# Palpo: A Rust Matrix Server Implementation

This is a matrix server project under development and is not yet available.

Some of the code comes from or references [ruma](https://github.com/ruma/ruma) and [conduit](https://gitlab.com/famedly/conduit).

Demo Server: https://matrix.palpo.im Go to https://app.cinny.in/ and input the demo server url to test.

## TODO List

- [ ] Complement tests `TestDeviceListUpdates/*`.
- [ ] Complement tests `TestE2EKeyBackupReplaceRoomKeyRules/*`.
- [ ] Complement tests `TestDeviceListsUpdateOverFederation/*`.
- [ ] Complement tests `TestFederationRoomsInvite/*`.
- [ ] Complement tests `TestRoomMembers/*`.
- [ ] Complement tests `TestRoomState/*`.
- [ ] Complement tests `TestToDeviceMessagesOverFederation/*`.
- [ ] Search
- [ ] Fill missing previous events.
- [ ] Use redis as data cache to improve data access speed. 
- [ ] Support for Mysql, Sqlite.
- [ ] Fallback older versions when remote federation server does not support the target version protocol.

All Complement test reslts: [__test_all.result.jsonl](tests/results/__test_all.result.jsonl)