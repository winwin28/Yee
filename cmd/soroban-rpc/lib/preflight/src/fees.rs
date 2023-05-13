use ledger_storage;
use soroban_env_host::budget::Budget;
use soroban_env_host::fees::{
    compute_transaction_resource_fee, FeeConfiguration, TransactionResources,
};
use soroban_env_host::storage::{AccessType, Footprint, Storage, StorageMap};
use soroban_env_host::xdr;
use soroban_env_host::xdr::{
    DecoratedSignature, DiagnosticEvent, ExtensionPoint, InvokeHostFunctionOp, LedgerFootprint,
    LedgerKey, Memo, MuxedAccount, MuxedAccountMed25519, Operation, OperationBody, Preconditions,
    SequenceNumber, SignatureHint, SorobanResources, SorobanTransactionData, Transaction,
    TransactionExt, TransactionV1Envelope, Uint256, WriteXdr,
};
use std::cmp::max;
use std::convert::TryInto;
use std::error;

pub(crate) fn compute_transaction_data_and_min_fee(
    invoke_hf_op: &InvokeHostFunctionOp,
    snapshot_source: &ledger_storage::LedgerStorage,
    storage: &Storage,
    budget: &Budget,
    events: &Vec<DiagnosticEvent>,
) -> Result<(SorobanTransactionData, i64), Box<dyn error::Error>> {
    let soroban_resources = calculate_soroban_resources(snapshot_source, storage, budget, events)?;
    let fee_configuration = get_fee_configuration(snapshot_source)?;

    let read_write_entries = soroban_resources.footprint.read_write.as_vec().len() as u32;

    let transaction_resources = TransactionResources {
        instructions: soroban_resources.instructions,
        read_entries: soroban_resources.footprint.read_only.as_vec().len() as u32
            + read_write_entries,
        write_entries: read_write_entries,
        read_bytes: soroban_resources.read_bytes,
        write_bytes: soroban_resources.write_bytes,
        metadata_size_bytes: soroban_resources.extended_meta_data_size_bytes,
        // Note: we could get a better transaction size if the full transaction was passed down to libpreflight
        transaction_size_bytes: estimate_max_transaction_size(
            invoke_hf_op,
            &soroban_resources.footprint,
        )?,
    };
    let (min_fee, ref_fee) =
        compute_transaction_resource_fee(&transaction_resources, &fee_configuration);
    let transaction_data = SorobanTransactionData {
        resources: soroban_resources,
        refundable_fee: ref_fee,
        ext: ExtensionPoint::V0,
    };
    Ok((transaction_data, min_fee))
}

fn estimate_max_transaction_size(
    invoke_hf_op: &InvokeHostFunctionOp,
    fp: &LedgerFootprint,
) -> Result<u32, Box<dyn error::Error>> {
    let source = MuxedAccount::MuxedEd25519(MuxedAccountMed25519 {
        id: 0,
        ed25519: Uint256([0; 32]),
    });
    // generate the maximum memo size and signature size
    // TODO: is this being too conservative?
    let memo_text: Vec<u8> = [0; 28].into();
    // TODO: find a better way to do this:
    let mut signatures: Vec<DecoratedSignature> = vec![];
    let mut signatures_left = 20;
    while signatures_left > 0 {
        signatures.push(DecoratedSignature {
            hint: SignatureHint([0; 4]),
            signature: Default::default(),
        });
        signatures_left -= 1;
    }
    let envelope = TransactionV1Envelope {
        tx: Transaction {
            source_account: source.clone(),
            fee: 0,
            seq_num: SequenceNumber(0),
            cond: Preconditions::None,
            memo: Memo::Text(memo_text.try_into()?),
            operations: vec![Operation {
                source_account: Some(source),
                body: OperationBody::InvokeHostFunction(invoke_hf_op.clone()),
            }]
            .try_into()?,
            ext: TransactionExt::V1(SorobanTransactionData {
                resources: SorobanResources {
                    footprint: fp.clone(),
                    instructions: 0,
                    read_bytes: 0,
                    write_bytes: 0,
                    extended_meta_data_size_bytes: 0,
                },
                refundable_fee: 0,
                ext: ExtensionPoint::V0,
            }),
        },
        signatures: signatures.try_into()?,
    };

    let envelope_xdr = envelope.to_xdr()?;
    let envelope_size = envelope_xdr.len();

    // Add a 15% leeway
    let envelope_size = envelope_size * 115 / 100;
    Ok(envelope_size as u32)
}

fn calculate_soroban_resources(
    snapshot_source: &ledger_storage::LedgerStorage,
    storage: &Storage,
    budget: &Budget,
    events: &Vec<DiagnosticEvent>,
) -> Result<SorobanResources, Box<dyn error::Error>> {
    let fp = storage_footprint_to_ledger_footprint(&storage.footprint)?;
    /*
      readBytes = size(footprint.readOnly) + size(footprint.readWrite)
      writeBytes = size(storage.map[rw entries])
      metadataSize = readBytes(footprint.readWrite) + writeBytes + eventsSize
    */
    let original_write_ledger_entry_bytes =
        calculate_unmodified_ledger_entry_bytes(&fp.read_write.as_vec(), snapshot_source)?;
    let read_bytes =
        calculate_unmodified_ledger_entry_bytes(&fp.read_only.as_vec(), snapshot_source)?
            + original_write_ledger_entry_bytes;
    let write_bytes =
        calculate_modified_read_write_ledger_entry_bytes(&storage.footprint, &storage.map, budget)?;
    let meta_data_size_bytes =
        original_write_ledger_entry_bytes + write_bytes + calculate_event_size_bytes(events)?;

    // Add a 15% leeway with a minimum of 50k instructions
    let instructions = max(
        budget.get_cpu_insns_count() + 50000,
        budget.get_cpu_insns_count() * 115 / 100,
    );
    Ok(SorobanResources {
        footprint: fp,
        instructions: instructions as u32,
        read_bytes: read_bytes,
        write_bytes: write_bytes,
        extended_meta_data_size_bytes: meta_data_size_bytes,
    })
}

fn get_fee_configuration(
    _snapshot_source: &ledger_storage::LedgerStorage,
) -> Result<FeeConfiguration, Box<dyn error::Error>> {
    // TODO: (at least part of) these values should be obtained from the network's ConfigSetting LedgerEntries
    //       (instead of hardcoding them to the initial values in the network)

    // Taken from Stellar Core's InitialSorobanNetworkConfig in NetworkConfig.h
    Ok(FeeConfiguration {
        fee_per_instruction_increment: 100,
        fee_per_read_entry: 5000,
        fee_per_write_entry: 20000,
        fee_per_read_1kb: 1000,
        fee_per_write_1kb: 4000,
        fee_per_historical_1kb: 100,
        fee_per_metadata_1kb: 200,
        fee_per_propagate_1kb: 2000,
    })
}

fn calculate_modified_read_write_ledger_entry_bytes(
    footprint: &Footprint,
    storage_map: &StorageMap,
    budget: &Budget,
) -> Result<u32, Box<dyn error::Error>> {
    let mut res: u32 = 0;
    for (lk, ole) in storage_map {
        match footprint.0.get::<LedgerKey>(lk, budget)? {
            Some(AccessType::ReadOnly) => (),
            Some(AccessType::ReadWrite) => {
                if let Some(le) = ole {
                    let entry_bytes = le.to_xdr()?;
                    let key_bytes = lk.to_xdr()?;
                    res += (entry_bytes.len() + key_bytes.len()) as u32;
                }
            }
            None => return Err("storage ledger entry not found in footprint".into()),
        }
    }
    Ok(res)
}

fn calculate_unmodified_ledger_entry_bytes(
    ledger_entries: &Vec<LedgerKey>,
    snapshot_source: &ledger_storage::LedgerStorage,
) -> Result<u32, Box<dyn error::Error>> {
    let mut res: u32 = 0;
    for lk in ledger_entries {
        res += lk.to_xdr()?.len() as u32;
        match snapshot_source.get_xdr(lk) {
            Ok(entry_bytes) => {
                res += entry_bytes.len() as u32;
            }
            Err(e) => {
                match e {
                    ledger_storage::Error::NotFound =>
                    // The entry is not present in the unmodified ledger storage.
                    // We assume it to be due to the entry being created by a host function invocation.
                    // Thus, we shouldn't count it in as unmodified.
                    {
                        continue;
                    }
                    _ => return Err(e)?,
                }
            }
        };
    }
    Ok(res)
}

fn calculate_event_size_bytes(events: &Vec<DiagnosticEvent>) -> Result<u32, xdr::Error> {
    let mut res: u32 = 0;
    for e in events {
        let event_xdr = e.to_xdr()?;
        res += event_xdr.len() as u32;
    }
    Ok(res)
}

fn storage_footprint_to_ledger_footprint(foot: &Footprint) -> Result<LedgerFootprint, xdr::Error> {
    let mut read_only: Vec<LedgerKey> = Vec::new();
    let mut read_write: Vec<LedgerKey> = Vec::new();
    for (k, v) in &foot.0 {
        match v {
            AccessType::ReadOnly => read_only.push((**k).clone()),
            AccessType::ReadWrite => read_write.push((**k).clone()),
        }
    }
    Ok(LedgerFootprint {
        read_only: read_only.try_into()?,
        read_write: read_write.try_into()?,
    })
}
