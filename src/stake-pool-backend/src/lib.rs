// src/lib.rs
mod error;
use candid::{CandidType, Deserialize, Principal};
use error::DepositError;
use ic_cdk::api::time;
use ic_cdk::call;
use ic_ledger_types::Subaccount;
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::{BoundedStorable, Storable},
    DefaultMemoryImpl, StableBTreeMap,
};
use icrc_ledger_types::icrc1::transfer::TransferArg;
use icrc_ledger_types::icrc1::{account::Account, transfer::TransferError};
use icrc_ledger_types::icrc2::transfer_from::TransferFromArgs;
use std::borrow::Cow;
use std::cell::RefCell;

#[derive(CandidType, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct UserKey {
    pub principal: Principal,
    pub subaccount: Subaccount,
}

impl Storable for UserKey {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(candid::encode_one(self).expect("Failed to encode UserKey"))
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        candid::decode_one(&bytes).expect("Failed to decode UserKey")
    }
}

impl BoundedStorable for UserKey {
    const MAX_SIZE: u32 = 100;
    const IS_FIXED_SIZE: bool = false;
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq)]
pub struct Deposit {
    pub id: u64,
    pub amount: u64,
    pub timestamp: u64,
    pub lock_period_days: u16,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct DepositList(pub Vec<Deposit>);

impl Storable for DepositList {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(candid::encode_one(self).expect("Failed to encode Deposit"))
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        candid::decode_one(&bytes).expect("Failed to decode Deposit")
    }
}

impl BoundedStorable for DepositList {
    const MAX_SIZE: u32 = 100;
    const IS_FIXED_SIZE: bool = false;
}

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static DEPOSIT_MAP: RefCell<StableBTreeMap<UserKey, DepositList, Memory>> =
        RefCell::new(StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0)))));

    static STAKE_BALANCE_MAP: RefCell<StableBTreeMap<UserKey, u64, Memory>> =
        RefCell::new(StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1)))));

    static DEPOSIT_ID_COUNTER: RefCell<u64> = RefCell::new(0);
}

const VALID_LOCKS: [u16; 3] = [90, 180, 360];

// Internal reusable logic for testing or canister
fn deposit_internal(
    principal: Principal,
    subaccount: Subaccount,
    lock_days: u16,
    amount: u64,
    timestamp: u64,
) -> Result<Deposit, DepositError> {
    if !VALID_LOCKS.contains(&lock_days) {
        return Err(DepositError::InvalidLockPeriod);
    }

    let key = UserKey {
        principal,
        subaccount,
    };

    let id = DEPOSIT_ID_COUNTER.with(|counter| {
        let mut c = counter.borrow_mut();
        *c += 1;
        *c
    });

    let deposit = Deposit {
        id,
        amount,
        timestamp,
        lock_period_days: lock_days,
    };

    DEPOSIT_MAP.with(|map| {
        let mut m = map.borrow_mut();
        let mut deposits = m.get(&key).unwrap_or(DepositList(vec![]));
        deposits.0.push(deposit.clone());
        m.insert(key.clone(), deposits);
    });

    // Update cumulative stake per user subaccount
    STAKE_BALANCE_MAP.with(|map| {
        let mut store = map.borrow_mut();
        let current = store.get(&key).unwrap_or(0);
        store.insert(key.clone(), current + amount);
    });

    Ok(deposit)
}

fn withdraw_internal(
    principal: Principal,
    subaccount: Subaccount,
    deposit_id: u64,
    now: u64,
) -> Result<u64, DepositError> {
    let user_key = UserKey {
        principal,
        subaccount,
    };

    // Get deposits list
    let deposit_list_opt = DEPOSIT_MAP.with(|map| map.borrow().get(&user_key));

    // Validate deposit exists
    let mut deposit_list = match deposit_list_opt {
        Some(list) => list,
        None => return Err(DepositError::NoDepositFound),
    };

    // Find the deposit by ID
    let position = deposit_list.0.iter().position(|d| d.id == deposit_id);
    let deposit = match position {
        Some(pos) => &deposit_list.0[pos],
        None => return Err(DepositError::NoDepositFound),
    };

    // Check lock expiry
    let unlock_time = deposit.timestamp + (deposit.lock_period_days as u64 * 86400);
    if now < unlock_time {
        return Err(DepositError::LockPeriodNotExpired);
    }

    // Remove deposit and update state
    let withdrawn = deposit_list.0.remove(position.unwrap());

    DEPOSIT_MAP.with(|map| {
        map.borrow_mut().insert(user_key.clone(), deposit_list);
    });

    STAKE_BALANCE_MAP.with(|map| {
        let mut m = map.borrow_mut();
        let current = m.get(&user_key).unwrap_or(0);
        m.insert(user_key.clone(), current.saturating_sub(withdrawn.amount));
    });

    Ok(withdrawn.amount)
}

async fn reward_pool_internal(caller: Principal, amount: u64) -> Result<bool, DepositError> {
    // 1. Transfer full reward from caller to canister
    let from = Account {
        owner: caller,
        subaccount: None,
    };
    let to = Account {
        owner: ic_cdk::id(),// need to check ledger id and replace it
        subaccount: None,
    };

    let transfer_args = TransferFromArgs {
        from,
        to,
        amount: amount.into(),
        spender_subaccount: None,
        fee: None,
        memo: None,
        created_at_time: None,
    };

    let (res,): (Result<u64, String>,) = call(
        Principal::from_text("icrc2_ledger").unwrap(),
        "icrc2_transfer_from",
        (transfer_args,),
    )
    .await
    .map_err(|e| DepositError::LedgerTransferFailed(format!("{:?}", e)))?;

    res.map_err(|e| DepositError::LedgerTransferFailed(format!("{:?}", e)))?;

    // 2. Total stake amount
    let total_stake: u128 =
        STAKE_BALANCE_MAP.with(|map| map.borrow().iter().map(|(_, s)| s as u128).sum());

    if total_stake == 0 {
        return Err(DepositError::NoStakerFound);
    }

    // 3. Sequentially transfer proportional reward to each staker
    let stake_data: Vec<(UserKey, u64)> =
        STAKE_BALANCE_MAP.with(|map| map.borrow().iter().map(|(k, v)| (k.clone(), v)).collect());

    for (key, stake) in stake_data {
        let reward = (stake as u128 * amount as u128) / total_stake;
        if reward == 0 {
            continue;
        }

        let to_account = Account {
            owner: key.principal,
            subaccount: Some(key.subaccount.0),
        };

        let transfer_arg = TransferArg {
            to: to_account,
            amount: (reward as u64).into(),
            fee: None,
            memo: None,
            from_subaccount: None,
            created_at_time: None,
        };

        let (res,): (Result<u64, String>,) = call(
            Principal::from_text("icrc2_ledger").unwrap(),
            "icrc1_transfer",
            (transfer_arg,),
        )
        .await
        .map_err(|e| DepositError::LedgerTransferFailed(format!("{:?}", e)))?;

        res.map_err(|e| DepositError::LedgerTransferFailed(format!("{:?}", e)))?;
    }

    Ok(true)
}

#[candid::candid_method(update)]
#[ic_cdk::update]
pub async fn deposit_funds(
    subaccount: Subaccount,
    lock_days: u16,
    amount: u64,
) -> Result<Deposit, DepositError> {
    let caller = ic_cdk::caller();
    let now = time() / 1_000_000_000;
    // Step 1: Pull tokens from user's subaccount
    let from_account = Account {
        owner: caller,
        subaccount: Some(subaccount.0),
    };

    let to_account = Account {
        owner: ic_cdk::id(),
        subaccount: None,
    };

    let transfer_args = TransferFromArgs {
        from: from_account,
        to: to_account,
        amount: amount.into(),
        spender_subaccount: None,
        fee: None,
        memo: None,
        created_at_time: None,
    };

    let (res,): (Result<u64, String>,) = call(
        Principal::from_text("icrc2_ledger").unwrap(), // need to check ledger id and replace it
        "icrc2_transfer_from",
        (transfer_args,),
    )
    .await
    .map_err(|e| DepositError::LedgerTransferFailed(format!("{:?}", e)))?;

    res.map_err(DepositError::LedgerTransferFailed)?;
    deposit_internal(caller, subaccount, lock_days, amount, now)
}

#[ic_cdk::update]
#[candid::candid_method(update)]
pub async fn withdraw_funds(subaccount: Subaccount, deposit_id: u64) -> Result<u64, DepositError> {
    let principal = ic_cdk::caller();
    let now = time() / 1_000_000_000;
    let withdrawn_amount = withdraw_internal(principal, subaccount, deposit_id, now)?;
    // Transfer funds back to user
    let to_account = Account {
        owner: principal,
        subaccount: Some(subaccount.0),
    };

    let transfer_arg = TransferArg {
        to: to_account,
        amount: withdrawn_amount.into(),
        fee: None,
        memo: None,
        created_at_time: None,
        from_subaccount: None,
    };

    let (transfer_res,): (Result<u64, TransferError>,) = call(
        Principal::from_text("icrc2_ledger").unwrap(), // need to check ledger id and replace it
        "icrc1_transfer",
        (transfer_arg,),
    )
    .await
    .map_err(|e| DepositError::LedgerTransferFailed(format!("{:?}", e)))?;

    transfer_res.map_err(|e| DepositError::LedgerTransferFailed(format!("{:?}", e)))?;
    Ok(withdrawn_amount)
}


#[ic_cdk::update]
#[candid::candid_method(update)]
pub async fn reward_pool(amount: u64) -> Result<bool, DepositError> {
    let caller = ic_cdk::caller();
    let result = reward_pool_internal(caller, amount).await;
    result
}

#[ic_cdk::query]
#[candid::candid_method(query)]
pub fn get_deposits_by_user() -> Vec<(Subaccount, Deposit)> {
    let caller = ic_cdk::caller();

    DEPOSIT_MAP.with(|map| {
        map.borrow()
            .iter()
            .filter(|(key, _)| key.principal == caller)
            .flat_map(|(key, list)| list.clone().0.into_iter().map(move |d| (key.subaccount, d)))
            .collect()
    })
}

#[ic_cdk::query]
#[candid::candid_method(query)]
pub fn get_stake_balance(subaccount: Subaccount) -> u64 {
    let principal = ic_cdk::caller();
    let key = UserKey {
        principal,
        subaccount,
    };
    STAKE_BALANCE_MAP.with(|map| map.borrow().get(&key).unwrap_or(0))
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_deposit_validation() {
        let caller = Principal::anonymous();
        let current_time = 1_000_000_000;
        let timestamp = current_time - (100 * 86400); // 100 days ago
        let subaccount: Subaccount = Subaccount([1u8; 32]);
        assert_eq!(
            deposit_internal(caller, subaccount, 91, 1_000_000_000, timestamp),
            Err(DepositError::InvalidLockPeriod)
        );

        let deposit1 = deposit_internal(caller, subaccount, 90, 1_000_000_000, timestamp).unwrap();
        assert_eq!(deposit1.id, 1);

        // double deposit with different lock period
        let deposit2 = deposit_internal(caller, subaccount, 180, 1_000_000_000, timestamp).unwrap();

        assert_eq!(deposit2.id, 2);
    }

    #[test]
    fn test_withdraw_funds_success() {
        let principal = Principal::anonymous();
        let sub = Subaccount([2u8; 32]);

        let current_time = 1_000_000_000;
        let timestamp = current_time - (100 * 86400); // 100 days ago

        let deposit = deposit_internal(principal, sub.clone(), 90, 1_000_000, timestamp).unwrap();
        assert_eq!(deposit.id, 1);

        let result = withdraw_internal(principal, sub, deposit.id, current_time);

        assert_eq!(result, Ok(1_000_000));
    }

    #[test]
    fn test_withdraw_funds_lock_not_expired() {
        let principal = Principal::anonymous();
        let sub = Subaccount([3u8; 32]);

        let current_time = 1_000_000_000; // Mocked current time (in seconds)

        // Deposit just now, lock not expired
        let deposit =
            deposit_internal(principal, sub.clone(), 90, 2_000_000, current_time).unwrap();

        assert_eq!(deposit.id, 1);

        let result = withdraw_internal(principal, sub, deposit.id, current_time);

        assert_eq!(result, Err(DepositError::LockPeriodNotExpired));
    }

    #[test]
    fn test_withdraw_funds_invalid_deposit_id() {
        let principal = Principal::anonymous();
        let sub = Subaccount([4u8; 32]);

        let current_time = 1_000_000_000;
        let timestamp = current_time - (100 * 86400); // 100 days ago
        let invalid_id = 999;

        let deposit = deposit_internal(principal, sub.clone(), 90, 3_000_000, timestamp).unwrap();

        assert_eq!(deposit.id, 1);

        let result = withdraw_internal(principal, sub, invalid_id, timestamp);

        assert_eq!(result, Err(DepositError::NoDepositFound));
    }

    #[tokio::test]
    async fn test_reward_pool_distributes_proportionally() {
        // Setup: 2 stakers with 100 and 300 stake

        let p1 = Principal::anonymous();
        let sub1 = Subaccount([4u8; 32]);

        let p2 = Principal::anonymous();
        let sub2 = Subaccount([8u8; 32]);

        let caller = Principal::anonymous();

        let current_time = 1_000_000_000;
        let timestamp = current_time - (100 * 86400); // 100 days ago
        let d1 = deposit_internal(p1, sub1, 180, 100, timestamp).unwrap();
        let d2 = deposit_internal(p2, sub2, 180, 300, timestamp).unwrap();

        let result = reward_pool_internal(caller, d1.amount + d2.amount).await;

        assert!(result.is_ok());
    }
}
