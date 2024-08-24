//! Loads and formats OP transaction RPC response.  

use std::{marker::PhantomData, sync::Arc};

use alloy_rpc_types::optimism::OptimismTransactionFields;
use op_alloy_network::{Network, Optimism};
use reth_evm_optimism::RethL1BlockInfo;
use reth_node_api::FullNodeComponents;
use reth_primitives::{TransactionSigned, TransactionSignedEcRecovered};
use reth_provider::{BlockReaderIdExt, TransactionsProvider};
use reth_rpc_eth_api::{
    helpers::{EthApiSpec, EthSigner, EthTransactions, LoadTransaction, SpawnBlocking},
    EthApiTypes, RawTransactionForwarder, TransactionCompat,
};
use reth_rpc_eth_types::EthStateCache;
use reth_rpc_types::TransactionInfo;
use revm::L1BlockInfo;

use crate::{OpEthApi, OpEthApiError};

impl<N, Eth> EthTransactions for OpEthApi<N, Eth>
where
    Self: LoadTransaction,
    N: FullNodeComponents,
{
    fn provider(&self) -> impl BlockReaderIdExt {
        self.inner.provider()
    }

    fn raw_tx_forwarder(&self) -> Option<Arc<dyn RawTransactionForwarder>> {
        self.inner.raw_tx_forwarder()
    }

    fn signers(&self) -> &parking_lot::RwLock<Vec<Box<dyn EthSigner>>> {
        self.inner.signers()
    }
}

impl<N, Eth> LoadTransaction for OpEthApi<N, Eth>
where
    Self: SpawnBlocking,
    N: FullNodeComponents,
{
    type Pool = N::Pool;

    fn provider(&self) -> impl TransactionsProvider {
        self.inner.provider()
    }

    fn cache(&self) -> &EthStateCache {
        self.inner.cache()
    }

    fn pool(&self) -> &Self::Pool {
        self.inner.pool()
    }
}

/// L1 fee and data gas for a transaction, along with the L1 block info.
#[derive(Debug, Default, Clone)]
pub struct OptimismTxMeta {
    /// The L1 block info.
    pub l1_block_info: Option<L1BlockInfo>,
    /// The L1 fee for the block.
    pub l1_fee: Option<u128>,
    /// The L1 data gas for the block.
    pub l1_data_gas: Option<u128>,
}

impl OptimismTxMeta {
    /// Creates a new [`OptimismTxMeta`].
    pub const fn new(
        l1_block_info: Option<L1BlockInfo>,
        l1_fee: Option<u128>,
        l1_data_gas: Option<u128>,
    ) -> Self {
        Self { l1_block_info, l1_fee, l1_data_gas }
    }
}

impl<N, Eth> OpEthApi<N, Eth>
where
    Self: EthApiSpec + LoadTransaction,
    <Self as EthApiTypes>::Error: From<OpEthApiError>,
    N: FullNodeComponents,
{
    /// Builds [`OptimismTxMeta`] object using the provided [`TransactionSigned`], L1 block
    /// info and block timestamp. The [`L1BlockInfo`] is used to calculate the l1 fee and l1 data
    /// gas for the transaction. If the [`L1BlockInfo`] is not provided, the meta info will be
    /// empty.
    pub fn build_op_tx_meta(
        &self,
        tx: &TransactionSigned,
        l1_block_info: Option<L1BlockInfo>,
        block_timestamp: u64,
    ) -> Result<OptimismTxMeta, <Self as EthApiTypes>::Error> {
        let Some(l1_block_info) = l1_block_info else { return Ok(OptimismTxMeta::default()) };

        let (l1_fee, l1_data_gas) = if !tx.is_deposit() {
            let envelope_buf = tx.envelope_encoded();

            let inner_l1_fee = l1_block_info
                .l1_tx_data_fee(&self.chain_spec(), block_timestamp, &envelope_buf, tx.is_deposit())
                .map_err(|_| OpEthApiError::L1BlockFeeError)?;
            let inner_l1_data_gas = l1_block_info
                .l1_data_gas(&self.chain_spec(), block_timestamp, &envelope_buf)
                .map_err(|_| OpEthApiError::L1BlockGasError)?;
            (
                Some(inner_l1_fee.saturating_to::<u128>()),
                Some(inner_l1_data_gas.saturating_to::<u128>()),
            )
        } else {
            (None, None)
        };

        Ok(OptimismTxMeta::new(Some(l1_block_info), l1_fee, l1_data_gas))
    }
}

/// Builds OP transaction response type.
#[derive(Debug, Clone, Copy)]
pub struct OpTxBuilder<Eth> {
    _l1_builders: PhantomData<Eth>,
}

impl<Eth: TransactionCompat<Transaction = <Optimism as Network>::TransactionResponse>>
    TransactionCompat for OpTxBuilder<Eth>
{
    type Transaction = <Optimism as Network>::TransactionResponse;

    fn fill(tx: TransactionSignedEcRecovered, tx_info: TransactionInfo) -> Self::Transaction {
        let signed_tx = tx.clone().into_signed();

        let mut resp = Eth::fill(tx, tx_info);

        resp.other = OptimismTransactionFields {
            source_hash: signed_tx.source_hash(),
            mint: signed_tx.mint(),
            is_system_tx: signed_tx.is_deposit().then_some(signed_tx.is_system_transaction()),
        }
        .into();

        resp
    }
}
