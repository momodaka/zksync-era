#![allow(clippy::derive_partial_eq_without_eq, clippy::format_push_string)]

use sqlx::{pool::PoolConnection, postgres::Postgres, Connection, PgConnection, Transaction};
pub use sqlx::{types::BigDecimal, Error as SqlxError};

pub use crate::connection::ConnectionPool;
use crate::{
    accounts_dal::AccountsDal, basic_witness_input_producer_dal::BasicWitnessInputProducerDal,
    blocks_dal::BlocksDal, blocks_web3_dal::BlocksWeb3Dal, connection::holder::ConnectionHolder,
    contract_verification_dal::ContractVerificationDal, eth_sender_dal::EthSenderDal,
    events_dal::EventsDal, events_web3_dal::EventsWeb3Dal,
    fri_gpu_prover_queue_dal::FriGpuProverQueueDal,
    fri_proof_compressor_dal::FriProofCompressorDal,
    fri_protocol_versions_dal::FriProtocolVersionsDal, fri_prover_dal::FriProverDal,
    fri_scheduler_dependency_tracker_dal::FriSchedulerDependencyTrackerDal,
    fri_witness_generator_dal::FriWitnessGeneratorDal, gpu_prover_queue_dal::GpuProverQueueDal,
    proof_generation_dal::ProofGenerationDal, protocol_versions_dal::ProtocolVersionsDal,
    protocol_versions_web3_dal::ProtocolVersionsWeb3Dal, prover_dal::ProverDal,
    snapshots_creator_dal::SnapshotsCreatorDal, snapshots_dal::SnapshotsDal,
    storage_dal::StorageDal, storage_logs_dal::StorageLogsDal,
    storage_logs_dedup_dal::StorageLogsDedupDal, storage_web3_dal::StorageWeb3Dal,
    sync_dal::SyncDal, system_dal::SystemDal, tokens_dal::TokensDal,
    tokens_web3_dal::TokensWeb3Dal, transactions_dal::TransactionsDal,
    transactions_web3_dal::TransactionsWeb3Dal,
};

#[macro_use]
mod macro_utils;
pub mod accounts_dal;
pub mod basic_witness_input_producer_dal;
pub mod blocks_dal;
pub mod blocks_web3_dal;
pub mod connection;
pub mod contract_verification_dal;
pub mod eth_sender_dal;
pub mod events_dal;
pub mod events_web3_dal;
pub mod fri_gpu_prover_queue_dal;
pub mod fri_proof_compressor_dal;
pub mod fri_protocol_versions_dal;
pub mod fri_prover_dal;
pub mod fri_scheduler_dependency_tracker_dal;
pub mod fri_witness_generator_dal;
pub mod gpu_prover_queue_dal;
pub mod healthcheck;
mod instrument;
mod metrics;
mod models;
pub mod proof_generation_dal;
pub mod protocol_versions_dal;
pub mod protocol_versions_web3_dal;
pub mod prover_dal;
pub mod snapshots_creator_dal;
pub mod snapshots_dal;
pub mod storage_dal;
pub mod storage_logs_dal;
pub mod storage_logs_dedup_dal;
pub mod storage_web3_dal;
pub mod sync_dal;
pub mod system_dal;
pub mod time_utils;
pub mod tokens_dal;
pub mod tokens_web3_dal;
pub mod transactions_dal;
pub mod transactions_web3_dal;

#[cfg(test)]
mod tests;

/// Storage processor is the main storage interaction point.
/// It holds down the connection (either direct or pooled) to the database
/// and provide methods to obtain different storage schema.
#[derive(Debug)]
pub struct StorageProcessor<'a> {
    conn: ConnectionHolder<'a>,
    in_transaction: bool,
}

impl<'a> StorageProcessor<'a> {
    pub async fn start_transaction<'c: 'b, 'b>(&'c mut self) -> sqlx::Result<StorageProcessor<'b>> {
        let transaction = self.conn().begin().await?;
        let mut processor = StorageProcessor::from_transaction(transaction);
        processor.in_transaction = true;
        Ok(processor)
    }

    /// Checks if the `StorageProcessor` is currently within database transaction.
    pub fn in_transaction(&self) -> bool {
        self.in_transaction
    }

    fn from_transaction(conn: Transaction<'a, Postgres>) -> Self {
        Self {
            conn: ConnectionHolder::Transaction(conn),
            in_transaction: true,
        }
    }

    pub async fn commit(self) -> sqlx::Result<()> {
        if let ConnectionHolder::Transaction(transaction) = self.conn {
            transaction.commit().await
        } else {
            panic!("StorageProcessor::commit can only be invoked after calling StorageProcessor::begin_transaction");
        }
    }

    /// Creates a `StorageProcessor` using a pool of connections.
    /// This method borrows one of the connections from the pool, and releases it
    /// after `drop`.
    pub(crate) fn from_pool(conn: PoolConnection<Postgres>) -> Self {
        Self {
            conn: ConnectionHolder::Pooled(conn),
            in_transaction: false,
        }
    }

    fn conn(&mut self) -> &mut PgConnection {
        match &mut self.conn {
            ConnectionHolder::Pooled(conn) => conn,
            ConnectionHolder::Transaction(conn) => conn,
        }
    }

    pub fn transactions_dal(&mut self) -> TransactionsDal<'_, 'a> {
        TransactionsDal { storage: self }
    }

    pub fn transactions_web3_dal(&mut self) -> TransactionsWeb3Dal<'_, 'a> {
        TransactionsWeb3Dal { storage: self }
    }

    pub fn accounts_dal(&mut self) -> AccountsDal<'_, 'a> {
        AccountsDal { storage: self }
    }

    pub fn basic_witness_input_producer_dal(&mut self) -> BasicWitnessInputProducerDal<'_, 'a> {
        BasicWitnessInputProducerDal { storage: self }
    }

    pub fn blocks_dal(&mut self) -> BlocksDal<'_, 'a> {
        BlocksDal { storage: self }
    }

    pub fn blocks_web3_dal(&mut self) -> BlocksWeb3Dal<'_, 'a> {
        BlocksWeb3Dal { storage: self }
    }

    pub fn eth_sender_dal(&mut self) -> EthSenderDal<'_, 'a> {
        EthSenderDal { storage: self }
    }

    pub fn events_dal(&mut self) -> EventsDal<'_, 'a> {
        EventsDal { storage: self }
    }

    pub fn events_web3_dal(&mut self) -> EventsWeb3Dal<'_, 'a> {
        EventsWeb3Dal { storage: self }
    }

    pub fn storage_dal(&mut self) -> StorageDal<'_, 'a> {
        StorageDal { storage: self }
    }

    pub fn storage_web3_dal(&mut self) -> StorageWeb3Dal<'_, 'a> {
        StorageWeb3Dal { storage: self }
    }

    pub fn storage_logs_dal(&mut self) -> StorageLogsDal<'_, 'a> {
        StorageLogsDal { storage: self }
    }

    pub fn storage_logs_dedup_dal(&mut self) -> StorageLogsDedupDal<'_, 'a> {
        StorageLogsDedupDal { storage: self }
    }

    pub fn tokens_dal(&mut self) -> TokensDal<'_, 'a> {
        TokensDal { storage: self }
    }

    pub fn tokens_web3_dal(&mut self) -> TokensWeb3Dal<'_, 'a> {
        TokensWeb3Dal { storage: self }
    }

    pub fn prover_dal(&mut self) -> ProverDal<'_, 'a> {
        ProverDal { storage: self }
    }

    pub fn contract_verification_dal(&mut self) -> ContractVerificationDal<'_, 'a> {
        ContractVerificationDal { storage: self }
    }

    pub fn gpu_prover_queue_dal(&mut self) -> GpuProverQueueDal<'_, 'a> {
        GpuProverQueueDal { storage: self }
    }

    pub fn protocol_versions_dal(&mut self) -> ProtocolVersionsDal<'_, 'a> {
        ProtocolVersionsDal { storage: self }
    }

    pub fn protocol_versions_web3_dal(&mut self) -> ProtocolVersionsWeb3Dal<'_, 'a> {
        ProtocolVersionsWeb3Dal { storage: self }
    }

    pub fn fri_witness_generator_dal(&mut self) -> FriWitnessGeneratorDal<'_, 'a> {
        FriWitnessGeneratorDal { storage: self }
    }

    pub fn fri_prover_jobs_dal(&mut self) -> FriProverDal<'_, 'a> {
        FriProverDal { storage: self }
    }

    pub fn sync_dal(&mut self) -> SyncDal<'_, 'a> {
        SyncDal { storage: self }
    }

    pub fn fri_scheduler_dependency_tracker_dal(
        &mut self,
    ) -> FriSchedulerDependencyTrackerDal<'_, 'a> {
        FriSchedulerDependencyTrackerDal { storage: self }
    }

    pub fn proof_generation_dal(&mut self) -> ProofGenerationDal<'_, 'a> {
        ProofGenerationDal { storage: self }
    }

    pub fn fri_gpu_prover_queue_dal(&mut self) -> FriGpuProverQueueDal<'_, 'a> {
        FriGpuProverQueueDal { storage: self }
    }

    pub fn fri_protocol_versions_dal(&mut self) -> FriProtocolVersionsDal<'_, 'a> {
        FriProtocolVersionsDal { storage: self }
    }

    pub fn fri_proof_compressor_dal(&mut self) -> FriProofCompressorDal<'_, 'a> {
        FriProofCompressorDal { storage: self }
    }

    pub fn system_dal(&mut self) -> SystemDal<'_, 'a> {
        SystemDal { storage: self }
    }

    pub fn snapshots_dal(&mut self) -> SnapshotsDal<'_, 'a> {
        SnapshotsDal { storage: self }
    }

    pub fn snapshots_creator_dal(&mut self) -> SnapshotsCreatorDal<'_, 'a> {
        SnapshotsCreatorDal { storage: self }
    }
}
