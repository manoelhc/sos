To create a tool that mirrors the "completeness" of `nftables` while using a YAML-based configuration, your AI Coder needs to understand that `nftables` isn't just a list of rules; it is a **state machine** and a **bytecode interpreter**.

Here is a comprehensive prompt you can provide to an AI Coder to build `sos-pf`.

---

## AI Coder Prompt: Architecting `sos-pf`

**Role:** You are a Senior Systems Engineer specializing in Linux Kernel Networking and eBPF.
**Objective:** Design and implement a packet filtering framework named `sos-pf`. This tool must act as a high-level wrapper and management engine for `nftables` (using `libnftables`), utilizing **YAML** as the exclusive configuration format.

### 1. Core Logic & Architecture

The tool must implement the **NFT Virtual Machine** logic. Every YAML configuration must be translated into an atomic transaction.

* **Atomic Batching:** Ensure rules are applied in a single "all-or-nothing" transaction to prevent network state leakage.
* **Family Support:** Implement support for `ip`, `ip6`, `inet` (unified), `arp`, `bridge`, and `netdev` families.
* **Hook Integration:** Support all Netfilter hooks: `prerouting`, `input`, `forward`, `output`, and `postrouting`.

### 2. Required Feature Modules

Implement the following `nftables` primitives within the YAML schema:

* **Sets & Maps:** Support for named sets (IP blacklists) and maps (verdict maps for high-speed lookups).
* **Stateful Inspection:** Implementation of `ct` (conntrack) for stateful tracking (`established`, `related`, `new`).
* **Payload Expression:** Capability to match headers for Ethernet, IPv4/6, TCP, UDP, ICMP, and SCTP.
* **Advanced Actions:** Support for `snat`, `dnat`, `masquerade`, `redirect`, `log`, and `reject`.
* **Meters & Limits:** Implement rate-limiting per IP/subnet using dynamic sets (e.g., preventing DDoS via `limit rate`).

### 3. The YAML Schema Specification

The configuration must follow this hierarchical structure:

```yaml
sos-pf:
  tables:
    - name: filter_table
      family: inet
      chains:
        - name: input_filter
          type: filter
          hook: input
          priority: 0
          policy: drop
          rules:
            - match: { ct: { state: [established, related] } }
              action: accept
            - match: { tcp: { dport: 22 } }
              action: accept
              comment: "Allow SSH"

```

### 4. Technical Requirements (The Build)

* **Backend:** Use C or Go. If using Go, use the `google/nftables` library. If using C, interface directly with `libnftables`.
* **Validation:** Implement a "dry-run" feature (`sos-pf check`) that validates YAML syntax and cross-references it against the running kernel's capabilities.
* **Observability:** Create a command to export the current kernel state back into the `sos-pf` YAML format.
* **Performance:** Use **Dictionary/Map** logic for port/IP matching to ensure $O(1)$ lookup performance rather than linear $O(n)$ scanning.

---

### How would you like to proceed?

I can help you **write the Go/C boilerplate code** for the YAML parser, or I can generate a **more complex YAML example** showing advanced features like Load Balancing and Port Forwarding. Which would you prefer?