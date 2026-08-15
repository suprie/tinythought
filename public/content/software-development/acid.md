---
title: "#ACID"
slug: acid
categories: ["software-development"]
tags: ["databases", "fundamentals"]
draft: false
---
Atomicity, Consistency, Isolation, Durability — the four guarantees that keep
database transactions honest.

A transaction either happens completely, or not at all. The database moves
from one valid state to another. Concurrent transactions don't see each
other's half-finished work. And once committed, it survives a crash.
