// src/lib.rs
mod error;
use candid::{CandidType, Deserialize, Principal};
use ic_cdk::api::time;
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::{BoundedStorable, Storable},
    DefaultMemoryImpl, StableBTreeMap,
};
use ic_ledger_types::Subaccount;
use std::borrow::Cow;
use std::cell::RefCell;
use error::DepositError;

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

#[derive(CandidType, Deserialize, Clone, Debug)]
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
) -> Result<(), DepositError> {
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
        deposits.0.push(deposit);
        m.insert(key.clone(), deposits);
    });

    // Update cumulative stake per user subaccount
    STAKE_BALANCE_MAP.with(|map| {
        let mut store = map.borrow_mut();
        let current = store.get(&key).unwrap_or(0);
        store.insert(key.clone(), current + amount);
    });

    Ok(())
}


#[candid::candid_method(update)]
#[ic_cdk::update]
pub fn deposit_funds(subaccount: Subaccount, lock_days: u16, amount: u64) -> Result<(), DepositError> {
    let caller = ic_cdk::caller();
    let now = time() / 1_000_000_000;
    deposit_internal(caller, subaccount, lock_days, amount, now)
}



#[ic_cdk::update]
#[candid::candid_method(update)]
pub fn withdraw_funds(subaccount: Subaccount, deposit_id: u64) -> Result<u64, DepositError> {
    let caller = ic_cdk::caller();
    let now = time() / 1_000_000_000;

    let user_key = UserKey { principal: caller, subaccount: subaccount };

    let mut found = None;

    DEPOSIT_MAP.with(|map| {
        let mut store = map.borrow_mut();
        if let Some(mut list) = store.get(&user_key) {
            if let Some(pos) = list.0.iter().position(|d| d.id == deposit_id) {
                found = Some(list.0.remove(pos));
                store.insert(user_key.clone(), list); // re-insert updated list
            }
        }
    });
    

    match found {
        Some(d) => {
            let unlock_time = d.timestamp + (d.lock_period_days as u64 * 86400);
            if now < unlock_time {
                // Reinsert if lock not expired
                DEPOSIT_MAP.with(|map| {
                    let mut list = map.borrow().get(&user_key).unwrap_or_else(|| DepositList(vec![]));
                    list.0.push(d.clone());
                    map.borrow_mut().insert(user_key.clone(), list);
                });
                return Err(DepositError::LockPeriodNotExpired);
            }

            STAKE_BALANCE_MAP.with(|map| {
                let mut m = map.borrow_mut();
                let current = m.get(&user_key).unwrap_or(0);
                m.insert(user_key.clone(), current.saturating_sub(d.amount));
            });

            Ok(d.amount)
        }
        None => Err(DepositError::NoDepositFound),
    }
}



#[ic_cdk::query]
#[candid::candid_method(query)]
pub fn get_deposits_by_user(caller: Principal) -> Vec<(Subaccount, Deposit)> {
    DEPOSIT_MAP.with(|map| {
        map.borrow()
            .iter()
            .filter(|(key, _)| key.principal == caller)
            .flat_map(|(key, list)| {
                list.clone()
                    .0
                    .into_iter()
                    .map(move |d| (key.subaccount, d))
            })
            .collect()
    })
}

#[ic_cdk::query]
#[candid::candid_method(query)]
pub fn get_stake_balance(subaccount: Subaccount) -> u64 {
    let principal = ic_cdk::caller();
    let key = UserKey { principal, subaccount };
    STAKE_BALANCE_MAP.with(|map| map.borrow().get(&key).unwrap_or(0))
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_deposit_validation() {
        let caller = Principal::anonymous();
        let timestamp = 0;

        let subaccount: Subaccount = Subaccount([1u8; 32]);
        assert_eq!(
            deposit_internal(caller, subaccount, 91, 1_000_000_000, timestamp),
            Err(DepositError::InvalidLockPeriod)
        );

        assert!(
            deposit_internal(caller, subaccount, 90, 1_000_000_000, timestamp).is_ok()
        );
    }

}
