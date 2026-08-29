# Digital Paper Companion — Product Documentation

Digital Paper Companion is a free, cross-platform desktop application for the
Sony Digital Paper devices DPT-RP1 and DPT-CP1 (and protocol-compatible
devices such as the Fujitsu Quaderno). It replaces Sony's discontinued
official "Digital Paper App".

This folder contains the complete product definition. A developer should be
able to build the application from these documents plus the protocol
specification.

## Document map

| Document | Contents | Audience |
|---|---|---|
| [01-product-overview.md](01-product-overview.md) | Vision, goals, target users, scope and non-goals, glossary | Everyone |
| [02-functional-requirements.md](02-functional-requirements.md) | Numbered functional requirements for every feature area | Product, developers, QA |
| [03-non-functional-requirements.md](03-non-functional-requirements.md) | Platforms, performance, security, reliability, i18n, accessibility | Developers, QA |
| [04-architecture.md](04-architecture.md) | Technical architecture: Tauri/Rust structure, crates, module layout, IPC contract | Developers |
| [05-ui-ux-specification.md](05-ui-ux-specification.md) | Screen inventory, navigation, interaction flows, design system | Designers, frontend developers |
| [06-sync-specification.md](06-sync-specification.md) | Two-way sync algorithm, checkpoint format, conflict rules, scheduling | Developers, QA |
| [07-data-and-security.md](07-data-and-security.md) | Local data storage, credential handling, TLS policy, threat model | Developers, security review |
| [08-roadmap.md](08-roadmap.md) | Release milestones and feature phasing | Everyone |
| [sony-digital-paper-protocol.md](sony-digital-paper-protocol.md) | Reverse-engineered device protocol (normative reference) | Developers |

## Reading order

- **New to the project?** Read 01, then skim 05 to understand what the app
  looks like, then 02.
- **Implementing a feature?** Read the relevant section of 02, then 04, and
  consult the protocol spec for the endpoints involved. For sync work, 06 is
  the authoritative specification.
- **Reviewing security?** Read 07 together with sections 4–5 of the protocol
  spec.

## Conventions

- Requirement IDs are stable and unique (`FR-CONN-3`, `NFR-SEC-2`, …) and are
  referenced from code reviews, tests and commit messages.
- "The device" always means a DPT-RP1/DPT-CP1 or compatible device; "the app"
  means Digital Paper Companion.
- RFC 2119 keywords (MUST, SHOULD, MAY) are used with their usual meaning.
