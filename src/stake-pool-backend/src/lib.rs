// src/lib.rs
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
    pub amount: u64,
    pub timestamp: u64,
    pub lock_period_days: u16,
}

impl Storable for Deposit {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(candid::encode_one(self).expect("Failed to encode Deposit"))
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        candid::decode_one(&bytes).expect("Failed to decode Deposit")
    }
}

impl BoundedStorable for Deposit {
    const MAX_SIZE: u32 = 100;
    const IS_FIXED_SIZE: bool = false;
}

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static DEPOSIT_MAP: RefCell<StableBTreeMap<UserKey, Deposit, Memory>> =
        RefCell::new(StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0)))));
}

const VALID_LOCKS: [u16; 3] = [90, 180, 360];

// Internal reusable logic for testing or canister
fn deposit_internal(
    principal: Principal,
    subaccount: Subaccount,
    lock_days: u16,
    amount: u64,
    timestamp: u64,
) -> Result<(), String> {
    if !VALID_LOCKS.contains(&lock_days) {
        return Err("Invalid lock period.".into());
    }

    let key = UserKey {
        principal,
        subaccount,
    };

    let deposit = Deposit {
        amount,
        timestamp,
        lock_period_days: lock_days,
    };

    DEPOSIT_MAP.with(|map| {
        map.borrow_mut().insert(key, deposit);
    });

    Ok(())
}

#[ic_cdk::update]
pub fn deposit_funds(subaccount: Subaccount, lock_days: u16, amount: u64) -> Result<(), String> {
    let caller = ic_cdk::caller();
    let now = time() / 1_000_000_000;
    deposit_internal(caller, subaccount, lock_days, amount, now)
}

#[ic_cdk::query]
pub fn get_deposits_by_user() -> Vec<(Subaccount, Deposit)> {
    let caller = ic_cdk::caller();
    DEPOSIT_MAP.with(|map| {
        map.borrow()
            .iter()
            .filter_map(|(key, deposit)| {
                if key.principal == caller {
                    Some((key.subaccount, deposit))
                } else {
                    None
                }
            })
            .collect()
    })
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
            Err("Invalid lock period.".to_string())
        );

        assert!(
            deposit_internal(caller, subaccount, 90, 1_000_000_000, timestamp).is_ok()
        );
    }

}
