# 🏦 ICP Staking Pool Canister

This is a Rust-based Internet Computer (IC) smart contract that provides a staking pool with:

- ✅ Time-locked deposits using subaccounts
- 💰 Reward distribution to stakers proportionally
- ⚔️ Slash pool to penalize stakers and send slashed funds to a receiver
- 🔁 ICRC-2 based ledger integration for real token transfers
- 🧪 Unit-tested internal logic for validation

---

## 🚀 Features

| Functionality     | Description |
|-------------------|-------------|
| `deposit_funds`   | Stake tokens with 90, 180, or 360-day lock |
| `withdraw_funds`  | Withdraw after lock period expires |
| `reward_pool`     | Transfer tokens to pool and distribute reward proportionally |
| `slash_pool`      | Deduct tokens from stakers and transfer to receiver |
| `get_deposits_by_user` | Query your deposits |
| `get_stake_balance`    | Get total staked balance for a subaccount |

---

## 📦 Tech Stack

- Rust Smart Contracts using `ic-cdk`
- Stable structures: `ic-stable-structures`
- ICRC-2/ICRC-1 token ledger support
- Testing via `cargo test` and `#[tokio::test]`

---

## 🧪 Testing Locally

### 1. Clone & Build

```bash
git clone https://github.com/akasharora963/icp-stake-pool.git
cd icp-stake-pool
cargo build
```

### 2. Run Unit Tests

```bash
cargo test
```

This tests:

- Lock duration validation
- Multiple deposits
- Withdraw scenarios (success, not expired, invalid ID)

---

## 🛠️ Deploy on Local Replica

### 1. Start IC Local Replica

```bash
dfx start --clean --background
```

### 2. Set Up Ledger Canister

Add to `dfx.json`:

```json
"icrc2_ledger": {
  "type": "custom",
  "candid": "path/to/icrc_ledger.did",
  "wasm": "path/to/icrc_ledger.wasm"
}
```

Then deploy:

```bash
dfx deploy icrc2_ledger
```

### 3. Deploy Your Staking Canister

```bash
dfx deploy
```

---

## ✍️ Example Usage

### Deposit Funds

```bash
dfx canister call staking_pool deposit_funds '(vec {1 : nat8; ... 32}, 90, 1000000)'
```

### Withdraw Funds

```bash
dfx canister call staking_pool withdraw_funds '(vec {1 : nat8; ... 32}, 1)'
```

### Reward Pool

```bash
dfx canister call staking_pool reward_pool '(500000)'
```

### Slash Pool

```bash
dfx canister call staking_pool slash_pool '(200000, record { principal = "<receiver>"; subaccount = vec {0 : nat8; ... 32} })'
```

---

## 📄 Internal Storage

| Key | Value |
|-----|-------|
| `UserKey` | (Principal, Subaccount) |
| `DepositList` | List of time-locked deposits |
| `STAKE_BALANCE_MAP` | Total staked amount per user |
| `DEPOSIT_ID_COUNTER` | Auto-incrementing deposit ID |

---

## ⚠️ Notes

- Ledger principal is hardcoded as `"icrc2_ledger"` – update with actual deployed principal.
- Time-based logic uses seconds (`ic_cdk::api::time()`).
- Subaccount must be exactly `[u8; 32]`.

---

## 🧾 Future Enhancements
- [] Testing of reward pool and slash pool
---



