# Palpo: A Rust Matrix Server Implementation

Palpo is a cutting-edge chat server written in Rust and supporting Matrix protocol and PostgreSQL database, aiming to deliver high performance, scalability, and robust federation capabilities. With Palpo, we aspire to redefine decentralized communication—providing real-time messaging and collaboration at minimal operational cost. We welcome open-source contributors and all kinds of help!

---

## Project Highlights

- **High-Performance Rust Core**  
  Based Salvo web server, Palpo leverages Rust’s safety and concurrency model, enabling a low-overhead server that is both fast and reliable.

- **Open Ecosystem**  
  Portions of our code reference or derive inspiration from the excellent work in [palpo](https://github.com/palpo/palpo) and [conduit](https://gitlab.com/famedly/conduit). By building atop established open-source projects, we aim for compatibility and rapid iteration.

- **Federation & Standards**  
  Palpo implements the Matrix protocol to ensure **full interoperability** with other Matrix homeservers, facilitating a **truly decentralized network** of real-time communication.

- **Demo Server**  
  - **URL**: [https://matrix.palpo.im](https://matrix.palpo.im)  
  - To test quickly, open [Cinny](https://app.cinny.in/) and use `https://matrix.palpo.im` as your custom homeserver.

---

## Current progress

We use [Complement](https://github.com/matrix-org/complement) for end-to-end testing.
All Complement test reslts: [test_all.result.jsonl](tests/results/test_all.result.jsonl)
