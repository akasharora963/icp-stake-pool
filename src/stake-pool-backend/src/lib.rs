// src/lib.rs
use candid::{CandidType, Deserialize, Principal};
use ic_cdk::api::time;
//use icrc_ledger_types::icrc1::account::Subaccount;
use ic_stable_structures::{
    memory_manager::{MemoryId,MemoryManager, VirtualMemory},
    storable::{BoundedStorable, Storable},
    DefaultMemoryImpl, StableBTreeMap,
};
use std::borrow::Cow;
use std::cell::RefCell;

#[derive(CandidType, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct UserKey {
    pub principal: Principal,
    pub subaccount: [u8; 32],
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
    subaccount: Vec<u8>,
    lock_days: u16,
    amount: u64,
    timestamp: u64,
) -> Result<(), String> {
    if subaccount.len() != 32 {
        return Err("Subaccount must be exactly 32 bytes.".into());
    }
    if !VALID_LOCKS.contains(&lock_days) {
        return Err("Invalid lock period.".into());
    }

    let mut fixed = [0u8; 32];
    fixed.copy_from_slice(&subaccount);

    let key = UserKey {
        principal,
        subaccount: fixed,
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
pub fn deposit_funds(subaccount: Vec<u8>, lock_days: u16, amount: u64) -> Result<(), String> {
    let caller = ic_cdk::caller();
    let now = time() / 1_000_000_000;
    deposit_internal(caller, subaccount, lock_days, amount, now)
}

#[ic_cdk::query]
pub fn get_deposits_by_user(caller:Principal) -> Vec<(Vec<u8>, Deposit)> {
    DEPOSIT_MAP.with(|map| {
        map.borrow()
            .iter()
            .filter_map(|(key, deposit)| {
                if key.principal == caller {
                    Some((key.subaccount.to_vec(), deposit))
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

        let subaccount = vec![1u8; 32];
        assert_eq!(
            deposit_internal(caller, subaccount.clone(), 91, 1_000_000_000, timestamp),
            Err("Invalid lock period.".to_string())
        );

        assert!(
            deposit_internal(caller, subaccount.clone(), 90, 1_000_000_000, timestamp)
                .is_ok()
        );
    }

    #[test]
    fn test_get_deposits_by_user() {
        let caller = Principal::anonymous();
        let timestamp = 0;        
        let subaccount = vec![1u8; 32]; 

        deposit_internal(caller, subaccount.clone(), 90, 1_000_000_000, timestamp).unwrap();
        assert_eq!(get_deposits_by_user(caller).len(), 1);
    }
}
