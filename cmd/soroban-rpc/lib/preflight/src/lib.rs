mod fees;
mod ledger_storage;

extern crate base64;
extern crate libc;
extern crate sha2;
extern crate soroban_env_host;

use ledger_storage::LedgerStorage;
use sha2::{Digest, Sha256};
use soroban_env_host::auth::RecordedAuthPayload;
use soroban_env_host::budget::Budget;
use soroban_env_host::events::Events;
use soroban_env_host::storage::Storage;
use soroban_env_host::xdr::{
    AccountId, ConfigSettingEntry, ConfigSettingId, DiagnosticEvent, InvokeHostFunctionOp,
    LedgerFootprint, OperationBody, ReadXdr, ScVal, SorobanAddressCredentials,
    SorobanAuthorizationEntry, SorobanCredentials, VecM, WriteXdr,
};
use soroban_env_host::{DiagnosticLevel, Host, LedgerInfo};
use std::convert::TryFrom;
use std::error::Error;
use std::ffi::{CStr, CString};
use std::panic;
use std::ptr::null_mut;
use std::rc::Rc;
use std::{error, mem};

#[repr(C)]
#[derive(Copy, Clone)]
pub struct CLedgerInfo {
    pub protocol_version: u32,
    pub sequence_number: u32,
    pub timestamp: u64,
    pub network_passphrase: *const libc::c_char,
    pub base_reserve: u32,
    pub min_temp_entry_expiration: u32,
    pub min_persistent_entry_expiration: u32,
    pub max_entry_expiration: u32,
    pub autobump_ledgers: u32,
}

impl From<CLedgerInfo> for LedgerInfo {
    fn from(c: CLedgerInfo) -> Self {
        let network_passphrase_cstr = unsafe { CStr::from_ptr(c.network_passphrase) };
        Self {
            protocol_version: c.protocol_version,
            sequence_number: c.sequence_number,
            timestamp: c.timestamp,
            network_id: Sha256::digest(network_passphrase_cstr.to_str().unwrap().as_bytes()).into(),
            base_reserve: c.base_reserve,
            min_temp_entry_expiration: c.min_temp_entry_expiration,
            min_persistent_entry_expiration: c.min_persistent_entry_expiration,
            max_entry_expiration: c.max_entry_expiration,
            autobump_ledgers: c.autobump_ledgers,
        }
    }
}

#[repr(C)]
pub struct CPreflightResult {
    pub error: *mut libc::c_char, // Error string in case of error, otherwise null
    pub auth: *mut *mut libc::c_char, // NULL terminated array of XDR SorobanAuthorizationEntrys in base64
    pub result: *mut libc::c_char,    // XDR SCVal in base64
    pub transaction_data: *mut libc::c_char, // SorobanTransactionData XDR in base64
    pub min_fee: i64,                 // Minimum recommended resource fee
    pub events: *mut *mut libc::c_char, // NULL terminated array of XDR ContractEvents in base64
    pub cpu_instructions: u64,
    pub memory_bytes: u64,
}

fn preflight_error(str: String) -> *mut CPreflightResult {
    let c_str = CString::new(str).unwrap();
    // transfer ownership to caller
    // caller needs to invoke free_preflight_result(result) when done
    Box::into_raw(Box::new(CPreflightResult {
        error: c_str.into_raw(),
        auth: null_mut(),
        result: null_mut(),
        transaction_data: null_mut(),
        min_fee: 0,
        events: null_mut(),
        cpu_instructions: 0,
        memory_bytes: 0,
    }))
}

#[no_mangle]
pub extern "C" fn preflight_invoke_hf_op(
    handle: libc::uintptr_t, // Go Handle to forward to SnapshotSourceGet and SnapshotSourceHas
    bucket_list_size: u64,   // Bucket list size for current ledger
    invoke_hf_op: *const libc::c_char, // InvokeHostFunctionOp XDR in base64
    source_account: *const libc::c_char, // AccountId XDR in base64
    ledger_info: CLedgerInfo,
) -> *mut CPreflightResult {
    catch_preflight_panic(Box::new(move || {
        preflight_invoke_hf_op_or_maybe_panic(
            handle,
            bucket_list_size,
            invoke_hf_op,
            source_account,
            ledger_info,
        )
    }))
}

fn preflight_invoke_hf_op_or_maybe_panic(
    handle: libc::uintptr_t,
    bucket_list_size: u64, // Go Handle to forward to SnapshotSourceGet and SnapshotSourceHas
    invoke_hf_op: *const libc::c_char, // InvokeHostFunctionOp XDR in base64
    source_account: *const libc::c_char, // AccountId XDR in base64
    ledger_info: CLedgerInfo,
) -> Result<CPreflightResult, Box<dyn error::Error>> {
    let invoke_hf_op_cstr = unsafe { CStr::from_ptr(invoke_hf_op) };
    let invoke_hf_op = InvokeHostFunctionOp::from_xdr_base64(invoke_hf_op_cstr.to_str()?)?;
    let source_account_cstr = unsafe { CStr::from_ptr(source_account) };
    let source_account = AccountId::from_xdr_base64(source_account_cstr.to_str()?)?;
    let storage = Storage::with_recording_footprint(Rc::new(LedgerStorage {
        golang_handle: handle,
    }));
    let budget = get_budget_from_network_config_params(&LedgerStorage {
        golang_handle: handle,
    })?;
    let host = Host::with_storage_and_budget(storage, budget);

    let needs_auth_recording = invoke_hf_op.auth.is_empty();
    if needs_auth_recording {
        host.switch_to_recording_auth()?;
    } else {
        host.set_authorization_entries(invoke_hf_op.auth.to_vec())?;
    }

    host.set_diagnostic_level(DiagnosticLevel::Debug)?;
    host.set_source_account(source_account)?;
    host.set_ledger_info(ledger_info.into())?;

    // Run the preflight.
    let result = host.invoke_function(invoke_hf_op.host_function.clone())?;
    let auths: VecM<SorobanAuthorizationEntry> = if needs_auth_recording {
        let payloads = host.get_recorded_auth_payloads()?;
        VecM::try_from(
            payloads
                .iter()
                .map(recorded_auth_payload_to_xdr)
                .collect::<Vec<_>>(),
        )?
    } else {
        invoke_hf_op.auth
    };

    let budget = host.budget_cloned();
    // Recover, convert and return the storage footprint and other values to C.
    let (storage, events) = host.try_finish()?;

    let diagnostic_events = host_events_to_diagnostic_events(&events);
    let (transaction_data, min_fee) = fees::compute_host_function_transaction_data_and_min_fee(
        &InvokeHostFunctionOp {
            host_function: invoke_hf_op.host_function,
            auth: auths.clone(),
        },
        &LedgerStorage {
            golang_handle: handle,
        },
        &storage,
        &budget,
        &diagnostic_events,
        bucket_list_size,
        ledger_info.sequence_number,
    )?;
    let transaction_data_cstr = CString::new(transaction_data.to_xdr_base64()?)?;
    Ok(CPreflightResult {
        error: null_mut(),
        auth: recorded_auth_payloads_to_c(auths.to_vec())?,
        result: CString::new(result.to_xdr_base64()?)?.into_raw(),
        transaction_data: transaction_data_cstr.into_raw(),
        min_fee,
        events: diagnostic_events_to_c(diagnostic_events)?,
        cpu_instructions: budget.get_cpu_insns_consumed()?,
        memory_bytes: budget.get_mem_bytes_consumed()?,
    })
}

fn get_budget_from_network_config_params(
    ledger_storage: &LedgerStorage,
) -> Result<Budget, Box<dyn error::Error>> {
    let ConfigSettingEntry::ContractComputeV0(compute) =
        ledger_storage.get_configuration_setting(ConfigSettingId::ContractComputeV0)?
    else {
        return Err(
            "get_budget_from_network_config_params((): unexpected config setting entry for ComputeV0 key".into(),
        );
    };

    let ConfigSettingEntry::ContractCostParamsCpuInstructions(cost_params_cpu) = ledger_storage
        .get_configuration_setting(ConfigSettingId::ContractCostParamsCpuInstructions)?
    else {
        return Err(
            "get_budget_from_network_config_params((): unexpected config setting entry for ComputeV0 key".into(),
        );
    };

    let ConfigSettingEntry::ContractCostParamsMemoryBytes(cost_params_memory) =
        ledger_storage.get_configuration_setting(ConfigSettingId::ContractCostParamsMemoryBytes)?
    else {
        return Err(
            "get_budget_from_network_config_params((): unexpected config setting entry for ComputeV0 key".into(),
        );
    };

    let budget = Budget::try_from_configs(
        compute.tx_max_instructions as u64,
        compute.tx_memory_limit as u64,
        cost_params_cpu,
        cost_params_memory,
    )?;
    Ok(budget)
}

#[no_mangle]
pub extern "C" fn preflight_footprint_expiration_op(
    handle: libc::uintptr_t, // Go Handle to forward to SnapshotSourceGet and SnapshotSourceHas
    bucket_list_size: u64,   // Bucket list size for current ledger
    op_body: *const libc::c_char, // OperationBody XDR in base64
    footprint: *const libc::c_char, // LedgerFootprint XDR in base64
    current_ledger_seq: u32,
) -> *mut CPreflightResult {
    catch_preflight_panic(Box::new(move || {
        preflight_footprint_expiration_op_or_maybe_panic(
            handle,
            bucket_list_size,
            op_body,
            footprint,
            current_ledger_seq,
        )
    }))
}

fn preflight_footprint_expiration_op_or_maybe_panic(
    handle: libc::uintptr_t,
    bucket_list_size: u64,
    op_body: *const libc::c_char,
    footprint: *const libc::c_char,
    current_ledger_seq: u32,
) -> Result<CPreflightResult, Box<dyn error::Error>> {
    let op_body_cstr = unsafe { CStr::from_ptr(op_body) };
    let op_body = OperationBody::from_xdr_base64(op_body_cstr.to_str()?)?;
    let footprint_cstr = unsafe { CStr::from_ptr(footprint) };
    let ledger_footprint = LedgerFootprint::from_xdr_base64(footprint_cstr.to_str()?)?;
    let ledger_storage = &ledger_storage::LedgerStorage {
        golang_handle: handle,
    };
    match op_body {
        OperationBody::BumpFootprintExpiration(op) => preflight_bump_footprint_expiration(
            ledger_footprint,
            op.ledgers_to_expire,
            ledger_storage,
            bucket_list_size,
            current_ledger_seq,
        ),
        OperationBody::RestoreFootprint(_) => preflight_restore_footprint(
            ledger_footprint,
            ledger_storage,
            bucket_list_size,
            current_ledger_seq,
        ),
        op => Err(format!(
            "preflight_footprint_expiration_op(): unsupported operation type {}",
            op.name()
        )
        .into()),
    }
}

fn preflight_bump_footprint_expiration(
    footprint: LedgerFootprint,
    ledgers_to_expire: u32,
    ledger_storage: &LedgerStorage,
    bucket_list_size: u64,
    current_ledger_seq: u32,
) -> Result<CPreflightResult, Box<dyn Error>> {
    let (transaction_data, min_fee) =
        fees::compute_bump_footprint_exp_transaction_data_and_min_fee(
            footprint,
            ledgers_to_expire,
            ledger_storage,
            bucket_list_size,
            current_ledger_seq,
        )?;
    let transaction_data_cstr = CString::new(transaction_data.to_xdr_base64()?)?;
    Ok(CPreflightResult {
        error: null_mut(),
        auth: null_mut(),
        result: null_mut(),
        transaction_data: transaction_data_cstr.into_raw(),
        min_fee,
        events: null_mut(),
        cpu_instructions: 0,
        memory_bytes: 0,
    })
}

fn preflight_restore_footprint(
    footprint: LedgerFootprint,
    ledger_storage: &LedgerStorage,
    bucket_list_size: u64,
    current_ledger_seq: u32,
) -> Result<CPreflightResult, Box<dyn Error>> {
    let (transaction_data, min_fee) = fees::compute_restore_footprint_transaction_data_and_min_fee(
        footprint,
        ledger_storage,
        bucket_list_size,
        current_ledger_seq,
    )?;
    let transaction_data_cstr = CString::new(transaction_data.to_xdr_base64()?)?;
    Ok(CPreflightResult {
        error: null_mut(),
        auth: null_mut(),
        result: null_mut(),
        transaction_data: transaction_data_cstr.into_raw(),
        min_fee,
        events: null_mut(),
        cpu_instructions: 0,
        memory_bytes: 0,
    })
}

fn catch_preflight_panic(
    op: Box<dyn Fn() -> Result<CPreflightResult, Box<dyn error::Error>>>,
) -> *mut CPreflightResult {
    // catch panics before they reach foreign callers (which otherwise would result in
    // undefined behavior)
    let res = panic::catch_unwind(panic::AssertUnwindSafe(|| op()));
    match res {
        Err(panic) => match panic.downcast::<String>() {
            Ok(panic_msg) => preflight_error(format!("panic during preflight() call: {panic_msg}")),
            Err(_) => preflight_error("panic during preflight() call: unknown cause".to_string()),
        },
        // transfer ownership to caller
        // caller needs to invoke free_preflight_result(result) when done
        Ok(r) => match r {
            Ok(r2) => Box::into_raw(Box::new(r2)),
            Err(e) => preflight_error(format!("{e}")),
        },
    }
}

fn recorded_auth_payloads_to_c(
    payloads: Vec<SorobanAuthorizationEntry>,
) -> Result<*mut *mut libc::c_char, Box<dyn error::Error>> {
    let xdr_base64_vec: Vec<String> = payloads
        .iter()
        .map(WriteXdr::to_xdr_base64)
        .collect::<Result<Vec<_>, _>>()?;
    string_vec_to_c_null_terminated_char_array(xdr_base64_vec)
}

fn recorded_auth_payload_to_xdr(payload: &RecordedAuthPayload) -> SorobanAuthorizationEntry {
    match (payload.address.clone(), payload.nonce) {
        (Some(address), Some(nonce)) => SorobanAuthorizationEntry {
            credentials: SorobanCredentials::Address(SorobanAddressCredentials {
                address,
                nonce,
                // signature is left empty. This is where the client will put their signatures when
                // submitting the transaction.
                signature_expiration_ledger: 0,
                signature: ScVal::Void,
            }),
            root_invocation: payload.invocation.clone(),
        },
        (None, None) => SorobanAuthorizationEntry {
            credentials: SorobanCredentials::SourceAccount,
            root_invocation: payload.invocation.clone(),
        },
        // the address and the nonce can't be present independently
        (a,n) =>
            panic!("recorded_auth_payload_to_xdr: address and nonce present independently (address: {:?}, nonce: {:?})", a, n),
    }
}

fn host_events_to_diagnostic_events(events: &Events) -> Vec<DiagnosticEvent> {
    let mut res: Vec<DiagnosticEvent> = Vec::new();
    for e in &events.0 {
        let diagnostic_event = DiagnosticEvent {
            in_successful_contract_call: !e.failed_call,
            event: e.event.clone(),
        };
        res.push(diagnostic_event);
    }
    res
}

fn diagnostic_events_to_c(
    events: Vec<DiagnosticEvent>,
) -> Result<*mut *mut libc::c_char, Box<dyn error::Error>> {
    let xdr_base64_vec: Vec<String> = events
        .iter()
        .map(DiagnosticEvent::to_xdr_base64)
        .collect::<Result<Vec<_>, _>>()?;
    string_vec_to_c_null_terminated_char_array(xdr_base64_vec)
}

fn string_vec_to_c_null_terminated_char_array(
    v: Vec<String>,
) -> Result<*mut *mut libc::c_char, Box<dyn error::Error>> {
    let mut out_vec: Vec<*mut libc::c_char> = Vec::new();
    for s in &v {
        let c_str = CString::new(s.clone())?.into_raw();
        out_vec.push(c_str);
    }

    // Add the ending NULL
    out_vec.push(null_mut());

    Ok(vec_to_c_array(out_vec))
}

fn vec_to_c_array<T>(mut v: Vec<T>) -> *mut T {
    // Make sure length and capacity are the same
    // (this allows using the length as the capacity when deallocating the vector)
    v.shrink_to_fit();
    assert_eq!(v.len(), v.capacity());

    // Get the pointer to our vector, we will deallocate it in free_c_null_terminated_char_array()
    // TODO: replace by `out_vec.into_raw_parts()` once the API stabilizes
    let ptr = v.as_mut_ptr();
    mem::forget(v);

    ptr
}

/// .
///
/// # Safety
///
/// .
#[no_mangle]
pub unsafe extern "C" fn free_preflight_result(result: *mut CPreflightResult) {
    if result.is_null() {
        return;
    }
    unsafe {
        if !(*result).error.is_null() {
            _ = CString::from_raw((*result).error);
        }

        if !(*result).auth.is_null() {
            free_c_null_terminated_char_array((*result).auth);
        }

        if !(*result).result.is_null() {
            _ = CString::from_raw((*result).result);
        }

        if !(*result).transaction_data.is_null() {
            _ = CString::from_raw((*result).transaction_data);
        }
        if !(*result).events.is_null() {
            free_c_null_terminated_char_array((*result).events);
        }
        _ = Box::from_raw(result);
    }
}

fn free_c_null_terminated_char_array(array: *mut *mut libc::c_char) {
    unsafe {
        // Iterate until we find a null value
        let mut i: usize = 0;
        loop {
            let c_char_ptr = *array.add(i);
            if c_char_ptr.is_null() {
                break;
            }
            // deallocate each string
            _ = CString::from_raw(c_char_ptr);
            i += 1;
        }
        // deallocate the containing vector
        _ = Vec::from_raw_parts(array, i + 1, i + 1);
    }
}
