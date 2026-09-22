# oracle-court

The dispute court for [oracle-prophecy](https://github.com/Baydashaaa/oracle-prophecy),
the prediction markets of Terra Oracle Classic.

When someone challenges a posted outcome, the market stops and waits for its
arbiter. This contract is that arbiter. Eligible voters decide YES, NO or VOID,
one vote each, and when voting ends anyone can close the case and send the
decision to the market.

## Who votes

Every vote weighs the same.

- **The council** votes on every dispute, unconditionally. While the protocol
  has few users, the council is the quorum.
- **Users** vote when a voting-power source says they may. That source is a
  separate contract, the planned Oracle Vault, queried with the moment the
  dispute began. Until one is configured, only the council votes.

## Who cannot vote, whatever their standing

The contract checks all three against the market itself:

- the **resolver**, who posted the reading under dispute;
- the **challenger**, who disputed it;
- **anyone with a stake** in that market.

In a multisig these would be promises. Here they are code.

## How a case runs

```
challenge in the market
  -> the first vote opens the case
  -> voting for voting_secs: YES / NO / VOID
  -> anyone calls close; the court sends Rule to the market
       clear majority          -> that decision
       tie at the top, or
       fewer votes than quorum -> VOID, everyone refunded
```

There is no separate "open" step. A dispute cannot be buried by nobody opening
it: the first vote opens it, and if nobody votes at all, the market voids the
dispute on its own when its arbiter window runs out.

`voting_secs` must be shorter than the market's `arbiter_secs`. The court
refuses to open a case otherwise, because a vote that outlasted the window
could never be delivered.

## The rules of a case are frozen when it opens

The council, the quorum and the voting-power source are copied into the case
at its first vote. Changing the config afterwards affects future cases only.
Otherwise an admin could add friendly voters, or raise the quorum, in the
middle of a dispute.

## Trust, plainly

- **The admin chooses the council** for future cases. That is real power, and
  it is the reason the rules of an open case are frozen. Moving the admin to
  the court itself, or to a DAO, is the long-term answer.
- **One vote per voter is only as strong as the gate in front of it.** With
  equal weight, capturing the court costs one eligible wallet times the votes
  needed for a majority. Keep the gate expensive enough that buying a majority
  costs more than the largest pot on the markets.
- **The court cannot verify the reading either.** Voters decide by looking at
  the chain; the contract records their decision.

## Deploying

The market needs the arbiter's address, and the court needs the market's.
So the market is created with a placeholder arbiter, the court is created
pointing at the market, and the market's arbiter is then updated:

```
oracle-prophecy UpdateConfig { arbiter: "<court address>", ... }
```

## Messages

| | who | what |
|---|---|---|
| `vote` | eligible voters | one vote per case; the first one opens it |
| `close` | **anyone** | after voting ends; sends the decision to the market |
| `update_config` | admin | future cases only; `vp_source: ""` removes the source |

| query | returns |
|---|---|
| `config` | the current rules |
| `case` | tallies, frozen rules, result |
| `vote` | how an address voted |
| `can_vote` | whether an address may vote now, and why not |

## The voting-power interface

Any contract that answers this query can serve as `vp_source`:

```json
{ "eligible": { "address": "terra1...", "at": 1790000000 } }
```
```json
{ "eligible": true }
```

`at` is the moment the dispute began. A source must only count positions
made before it, so voting power cannot be bought after a dispute opens.

## Building

```bash
cargo build

cd integration && cargo test
# 15 tests against the real oracle-prophecy contract.
# Needs ../../oracle-prophecy on the 0.2.0 branch next to this repo.
```

The contract crate itself depends on nothing outside this folder, so the
optimizer builds it as is:

```bash
docker run --rm -v "$(pwd)":/code \
  --mount type=volume,source="$(basename "$(pwd)")_cache",target=/target \
  --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
  cosmwasm/optimizer:0.16.0
```
