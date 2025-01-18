# Palpo: A Rust Matrix Server Implementation

Palpo is a cutting-edge chat server written in Rust and supporting Matrix protocol and PostgreSQL database, aiming to deliver high performance, scalability, and robust federation capabilities. With Palpo, we aspire to redefine decentralized communication—providing real-time messaging and collaboration at minimal operational cost. We welcome open-source contributors and all kinds of help!

---

## Project Highlights

- **High-Performance Rust Core**  
  Based Salvo web server, Palpo leverages Rust’s safety and concurrency model, enabling a low-overhead server that is both fast and reliable.

- **Open Ecosystem**  
  Portions of our code reference or derive inspiration from the excellent work in [ruma](https://github.com/ruma/ruma) and [conduit](https://gitlab.com/famedly/conduit). By building atop established open-source projects, we aim for compatibility and rapid iteration.

- **Federation & Standards**  
  Palpo implements the Matrix protocol to ensure **full interoperability** with other Matrix homeservers, facilitating a **truly decentralized network** of real-time communication.

- **Demo Server**  
  - **URL**: [https://matrix.palpo.im](https://matrix.palpo.im)  
  - To test quickly, open [Cinny](https://app.cinny.in/) and use `https://matrix.palpo.im` as your custom homeserver.

---

## Current progress
All Complement test reslts: [__test_all.result.jsonl](tests/results/__test_all.result.jsonl)

---

## 2025 TODO List

### Upcoming Features

- [ ] **Search**: Implement robust, indexed search for room history
- [ ] **Bug fixes**: Fill missing previous events  
- [ ] **Protocol fallback**: Support older server versions where remote federations can’t upgrade  
- [ ] **Sliding Sync**: Lightweight sync mechanism for better performance on mobile and web clients
- [ ] **SSO Identity Providers**: Integrate single sign-on flows for enterprise deployments  
- [ ] **Server Management**: Provide UI and CLI tools for config, monitoring, upgrades

### Database Layer

- [ ] **Multi-DB Support**: Add MySQL & SQLite integration  
- [ ] **Caching**: Use Redis for faster data reads/writes  
- [ ] **Main-Replica Setup**: Boost performance for high-traffic environments  
- [ ] **Documentation & Website**: Enhance developer docs and build an informative project site

### Major tests to be passed
- [ ] Complement tests `TestDeviceListUpdates/*`.
- [ ] Complement tests `TestE2EKeyBackupReplaceRoomKeyRules/*`.
- [ ] Complement tests `TestDeviceListsUpdateOverFederation/*`.
- [ ] Complement tests `TestFederationRoomsInvite/*`.
- [ ] Complement tests `TestRoomMembers/*`.
- [ ] Complement tests `TestRoomState/*`.
- [ ] Complement tests `TestToDeviceMessagesOverFederation/*`.
- [ ] Other complement tests.

We use [Complement](https://github.com/matrix-org/complement) for end-to-end testing. 
