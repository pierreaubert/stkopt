//! Shared helpers for batched dynamic storage reads.

use crate::error::ChainError;
use std::collections::HashMap;
use subxt::backend::{Backend, CombinedBackend};
use subxt::dynamic::Value;
use subxt::ext::frame_decode::storage::StorageTypeInfo;
use subxt::ext::scale_decode::IntoVisitor;
use subxt::utils::AccountId32;
use subxt::{OnlineClient, PolkadotConfig};

/// Stable map key for AccountId32-indexed storage maps.
pub(crate) fn account_key(address: &AccountId32) -> [u8; 32] {
    *AsRef::<[u8; 32]>::as_ref(address)
}

/// Fetch multiple AccountId32-indexed storage values in one backend request.
pub(crate) async fn fetch_account_storage_values(
    client: &OnlineClient<PolkadotConfig>,
    backend: &CombinedBackend<PolkadotConfig>,
    pallet_name: &str,
    entry_name: &str,
    addresses: &[AccountId32],
) -> Result<HashMap<[u8; 32], Value>, ChainError> {
    let block = client.at_current_block().await?;
    let block_hash = block.block_hash();
    let storage_query = subxt::dynamic::storage::<Vec<Value>, Value>(pallet_name, entry_name);
    let storage_entry = block.storage().entry(storage_query.clone())?;
    let storage_info = block
        .metadata_ref()
        .storage_info(pallet_name, entry_name)
        .map_err(|e| ChainError::InvalidData(e.into_owned().to_string()))?
        .into_owned();
    let types = block.metadata_ref().types().clone();

    let requests: Vec<([u8; 32], Vec<u8>)> = addresses
        .iter()
        .map(|address| {
            let key_parts = vec![Value::from_bytes(address.clone())];
            let key_bytes = storage_entry.fetch_key(key_parts)?;
            Ok((account_key(address), key_bytes))
        })
        .collect::<Result<_, subxt::Error>>()?;

    let keys = requests.iter().map(|(_, key)| key.clone()).collect();
    let mut responses = backend
        .storage_fetch_values(keys, block_hash)
        .await
        .map_err(|e| ChainError::Subxt(subxt::Error::from(e)))?;

    let by_storage_key: HashMap<Vec<u8>, [u8; 32]> = requests
        .into_iter()
        .map(|(address, key_bytes)| (key_bytes, address))
        .collect();
    let mut values = HashMap::new();

    while let Some(response) = responses.next().await {
        let response = response.map_err(|e| ChainError::Subxt(subxt::Error::from(e)))?;
        let Some(address) = by_storage_key.get(&response.key) else {
            tracing::debug!(
                "Ignoring unexpected storage response key for {pallet_name}.{entry_name}"
            );
            continue;
        };
        let mut cursor = &response.value[..];
        let value = subxt::ext::frame_decode::storage::decode_storage_value_with_info(
            &mut cursor,
            &storage_info,
            &types,
            Value::into_visitor(),
        )
        .map_err(|e| ChainError::InvalidData(e.to_string()))?;
        if !cursor.is_empty() {
            return Err(ChainError::InvalidData(format!(
                "leftover bytes decoding {pallet_name}.{entry_name}: {} bytes",
                cursor.len()
            )));
        }
        values.insert(*address, value);
    }

    Ok(values)
}
