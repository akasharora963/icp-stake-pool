use candid::{CandidType, Deserialize, Principal};
use ic_cdk::api::time;
use ic_cdk_macros::*;
use icrc_ledger_types::icrc1::account::Subaccount;
use ic_stable_structures::{
    memory_manager::{MemoryManager, VirtualMemory},
    storable::{BoundedStorable, Storable},
    DefaultMemoryImpl, StableBTreeMap,
};
use std::borrow::Cow;
use std::cell::RefCell;

// ----------------------
// Types
// ----------------------

#[derive(CandidType, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
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

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
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

// ----------------------
// Stable Structures
// ----------------------

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    static MEMORY_MANAGER: MemoryManager<DefaultMemoryImpl> = MemoryManager::init(DefaultMemoryImpl::default());
    static DEPOSIT_MAP: RefCell<StableBTreeMap<UserKey, Deposit, Memory>> =
        RefCell::new(StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.get(0))));
}


