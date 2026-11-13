---
name: clarify
description: Grill the user relentlessly about a plan, decision, or idea. Use when the user wants to stress-test their thinking, or uses any 'grill' trigger phrases.
---

Interview the user relentlessly until you reach a shared understanding. Map this as a **design tree**: every decision branches into the decisions that hang off it.

Work the tree in **rounds**. The **frontier** is every decision whose prerequisites are already settled: the questions you can ask _now_ without guessing at answers you haven't heard yet. Ask the whole frontier in one round as a **single `question` tool call** containing every question, then wait for the user's answers before the next round.

Shape each question for the tool:

- `question`: the decision itself, with whatever context the user needs; multiple paragraphs are fine.
- `header`: `n· <short title>`, where `n` numbers questions continuously across the whole session (it never resets per round) and the header stays under 30 characters.
- `options`: always required, even for open-ended decisions: offer 2–4 candidate answers. Put your recommended answer first with "(Recommended)" appended to its label, and the reasoning for the recommendation in that option's `description`. The built-in "Type your own answer" covers everything else; never add your own catch-all option.
- `multiple`: leave it false unless the options genuinely combine.

A question that comes back "Unanswered" is not settled: re-ask it in the next round alongside the newly-unblocked questions.

Each round the user answers reshapes the tree: settled decisions push the frontier outward and unblock questions that depended on them. Recompute the frontier and ask the next round. A question whose answer depends on another question still open in this round belongs to a _later_ round, not this one.

Finding _facts_ is your job, never the user's. When a frontier question needs a fact from the environment (filesystem, tools, etc.), dispatch a sub-agent to find it; don't ask the user for anything you could look up yourself. Don't block on it: a running exploration is an unsettled prerequisite, so only the questions downstream of it wait for the sub-agent to report; ask the rest of the frontier now. The _decisions_ are the user's: put each to them and wait.

The session is done when the frontier is empty: every branch of the design tree visited, nothing left silently assumed. Do not act on it until the user confirms you have reached a shared understanding.
