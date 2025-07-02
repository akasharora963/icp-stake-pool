use candid::CandidType;

#[derive(CandidType, Debug, PartialEq)]
pub enum DepositError {
    InvalidLockPeriod,
    LockPeriodNotExpired,
    NoDepositFound,
    LedgerTransferFailed(String),
    NoStakerFound,
}
