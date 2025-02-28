use std::time::Duration;

use zksync_contracts::BaseSystemContractsHashes;
use zksync_types::{
    block::{L1BatchHeader, MiniblockHasher, MiniblockHeader},
    fee::{Fee, TransactionExecutionMetrics},
    helpers::unix_timestamp_ms,
    l1::{L1Tx, OpProcessingType, PriorityQueueType},
    l2::L2Tx,
    proofs::AggregationRound,
    tx::{tx_execution_info::TxExecutionStatus, ExecutionMetrics, TransactionExecutionResult},
    Address, Execute, L1BatchNumber, L1BlockNumber, L1TxCommonData, L2ChainId, MiniblockNumber,
    PriorityOpId, ProtocolVersion, ProtocolVersionId, H160, H256, MAX_GAS_PER_PUBDATA_BYTE, U256,
};

use crate::{
    blocks_dal::BlocksDal,
    connection::ConnectionPool,
    protocol_versions_dal::ProtocolVersionsDal,
    prover_dal::{GetProverJobsParams, ProverDal},
    transactions_dal::{L2TxSubmissionResult, TransactionsDal},
    transactions_web3_dal::TransactionsWeb3Dal,
};

const DEFAULT_GAS_PER_PUBDATA: u32 = 100;

fn mock_tx_execution_metrics() -> TransactionExecutionMetrics {
    TransactionExecutionMetrics::default()
}

pub(crate) fn create_miniblock_header(number: u32) -> MiniblockHeader {
    let number = MiniblockNumber(number);
    let protocol_version = ProtocolVersionId::default();
    MiniblockHeader {
        number,
        timestamp: 0,
        hash: MiniblockHasher::new(number, 0, H256::zero()).finalize(protocol_version),
        l1_tx_count: 0,
        l2_tx_count: 0,
        base_fee_per_gas: 100,
        l1_gas_price: 100,
        l2_fair_gas_price: 100,
        base_system_contracts_hashes: BaseSystemContractsHashes::default(),
        protocol_version: Some(protocol_version),
        virtual_blocks: 1,
    }
}

pub(crate) fn mock_l2_transaction() -> L2Tx {
    let fee = Fee {
        gas_limit: U256::from(1_000_000u32),
        max_fee_per_gas: U256::from(250_000_000u32),
        max_priority_fee_per_gas: U256::zero(),
        gas_per_pubdata_limit: U256::from(DEFAULT_GAS_PER_PUBDATA),
    };
    let mut l2_tx = L2Tx::new_signed(
        Address::random(),
        vec![],
        zksync_types::Nonce(0),
        fee,
        Default::default(),
        L2ChainId::from(270),
        &H256::random(),
        None,
        Default::default(),
    )
    .unwrap();

    l2_tx.set_input(H256::random().0.to_vec(), H256::random());
    l2_tx
}

fn mock_l1_execute() -> L1Tx {
    let serial_id = 1;
    let priority_op_data = L1TxCommonData {
        sender: H160::random(),
        canonical_tx_hash: H256::from_low_u64_be(serial_id),
        serial_id: PriorityOpId(serial_id),
        deadline_block: 100000,
        layer_2_tip_fee: U256::zero(),
        full_fee: U256::zero(),
        gas_limit: U256::from(100_100),
        max_fee_per_gas: U256::from(1u32),
        gas_per_pubdata_limit: MAX_GAS_PER_PUBDATA_BYTE.into(),
        op_processing_type: OpProcessingType::Common,
        priority_queue_type: PriorityQueueType::Deque,
        eth_hash: H256::random(),
        to_mint: U256::zero(),
        refund_recipient: Address::random(),
        eth_block: 1,
    };

    let execute = Execute {
        contract_address: H160::random(),
        value: Default::default(),
        calldata: vec![],
        factory_deps: None,
    };

    L1Tx {
        common_data: priority_op_data,
        execute,
        received_timestamp_ms: 0,
    }
}

pub(crate) fn mock_execution_result(transaction: L2Tx) -> TransactionExecutionResult {
    TransactionExecutionResult {
        hash: transaction.hash(),
        transaction: transaction.into(),
        execution_info: ExecutionMetrics::default(),
        execution_status: TxExecutionStatus::Success,
        refunded_gas: 0,
        operator_suggested_refund: 0,
        compressed_bytecodes: vec![],
        call_traces: vec![],
        revert_reason: None,
    }
}

#[tokio::test]
async fn workflow_with_submit_tx_equal_hashes() {
    let connection_pool = ConnectionPool::test_pool().await;
    let storage = &mut connection_pool.access_storage().await.unwrap();
    let mut transactions_dal = TransactionsDal { storage };

    let tx = mock_l2_transaction();
    let result = transactions_dal
        .insert_transaction_l2(tx.clone(), mock_tx_execution_metrics())
        .await;

    assert_eq!(result, L2TxSubmissionResult::Added);

    let result = transactions_dal
        .insert_transaction_l2(tx, mock_tx_execution_metrics())
        .await;

    assert_eq!(result, L2TxSubmissionResult::Replaced);
}

#[tokio::test]
async fn workflow_with_submit_tx_diff_hashes() {
    let connection_pool = ConnectionPool::test_pool().await;
    let storage = &mut connection_pool.access_storage().await.unwrap();
    let mut transactions_dal = TransactionsDal { storage };

    let tx = mock_l2_transaction();

    let nonce = tx.common_data.nonce;
    let initiator_address = tx.common_data.initiator_address;

    let result = transactions_dal
        .insert_transaction_l2(tx, mock_tx_execution_metrics())
        .await;

    assert_eq!(result, L2TxSubmissionResult::Added);

    let mut tx = mock_l2_transaction();
    tx.common_data.nonce = nonce;
    tx.common_data.initiator_address = initiator_address;
    let result = transactions_dal
        .insert_transaction_l2(tx, mock_tx_execution_metrics())
        .await;

    assert_eq!(result, L2TxSubmissionResult::Replaced);
}

#[tokio::test]
async fn remove_stuck_txs() {
    let connection_pool = ConnectionPool::test_pool().await;
    let storage = &mut connection_pool.access_storage().await.unwrap();
    let mut protocol_versions_dal = ProtocolVersionsDal { storage };
    protocol_versions_dal
        .save_protocol_version_with_tx(Default::default())
        .await;

    let storage = protocol_versions_dal.storage;
    let mut transactions_dal = TransactionsDal { storage };

    // Stuck tx
    let mut tx = mock_l2_transaction();
    tx.received_timestamp_ms = unix_timestamp_ms() - Duration::new(1000, 0).as_millis() as u64;
    transactions_dal
        .insert_transaction_l2(tx, mock_tx_execution_metrics())
        .await;
    // Tx in mempool
    let tx = mock_l2_transaction();
    transactions_dal
        .insert_transaction_l2(tx, mock_tx_execution_metrics())
        .await;

    // Stuck L1 tx. We should never ever remove L1 tx
    let mut tx = mock_l1_execute();
    tx.received_timestamp_ms = unix_timestamp_ms() - Duration::new(1000, 0).as_millis() as u64;
    transactions_dal
        .insert_transaction_l1(tx, L1BlockNumber(1))
        .await;

    // Old executed tx
    let mut executed_tx = mock_l2_transaction();
    executed_tx.received_timestamp_ms =
        unix_timestamp_ms() - Duration::new(1000, 0).as_millis() as u64;
    transactions_dal
        .insert_transaction_l2(executed_tx.clone(), mock_tx_execution_metrics())
        .await;

    // Get all txs
    transactions_dal.reset_mempool().await;
    let txs = transactions_dal
        .sync_mempool(vec![], vec![], 0, 0, 1000)
        .await
        .0;
    assert_eq!(txs.len(), 4);

    let storage = transactions_dal.storage;
    BlocksDal { storage }
        .insert_miniblock(&create_miniblock_header(1))
        .await
        .unwrap();

    let mut transactions_dal = TransactionsDal { storage };
    transactions_dal
        .mark_txs_as_executed_in_miniblock(
            MiniblockNumber(1),
            &[mock_execution_result(executed_tx.clone())],
            U256::from(1),
        )
        .await;

    // Get all txs
    transactions_dal.reset_mempool().await;
    let txs = transactions_dal
        .sync_mempool(vec![], vec![], 0, 0, 1000)
        .await
        .0;
    assert_eq!(txs.len(), 3);

    // Remove one stuck tx
    let removed_txs = transactions_dal
        .remove_stuck_txs(Duration::from_secs(500))
        .await;
    assert_eq!(removed_txs, 1);
    transactions_dal.reset_mempool().await;
    let txs = transactions_dal
        .sync_mempool(vec![], vec![], 0, 0, 1000)
        .await
        .0;
    assert_eq!(txs.len(), 2);

    // We shouldn't collect executed tx
    let storage = transactions_dal.storage;
    let mut transactions_web3_dal = TransactionsWeb3Dal { storage };
    transactions_web3_dal
        .get_transaction_receipt(executed_tx.hash())
        .await
        .unwrap()
        .unwrap();
}

fn create_circuits() -> Vec<(&'static str, String)> {
    vec![
        ("Main VM", "1_0_Main VM_BasicCircuits.bin".to_owned()),
        ("SHA256", "1_1_SHA256_BasicCircuits.bin".to_owned()),
        (
            "Code decommitter",
            "1_2_Code decommitter_BasicCircuits.bin".to_owned(),
        ),
        (
            "Log demuxer",
            "1_3_Log demuxer_BasicCircuits.bin".to_owned(),
        ),
    ]
}

#[tokio::test]
async fn test_duplicate_insert_prover_jobs() {
    let connection_pool = ConnectionPool::test_pool().await;
    let storage = &mut connection_pool.access_storage().await.unwrap();
    storage
        .protocol_versions_dal()
        .save_protocol_version_with_tx(Default::default())
        .await;
    storage
        .protocol_versions_dal()
        .save_prover_protocol_version(Default::default())
        .await;
    let block_number = 1;
    let header = L1BatchHeader::new(
        L1BatchNumber(block_number),
        0,
        Default::default(),
        Default::default(),
        Default::default(),
    );
    storage
        .blocks_dal()
        .insert_l1_batch(&header, &[], Default::default(), &[], &[])
        .await
        .unwrap();

    let mut prover_dal = ProverDal { storage };
    let circuits = create_circuits();
    let l1_batch_number = L1BatchNumber(block_number);
    prover_dal
        .insert_prover_jobs(
            l1_batch_number,
            circuits.clone(),
            AggregationRound::BasicCircuits,
            ProtocolVersionId::latest() as i32,
        )
        .await;

    // try inserting the same jobs again to ensure it does not panic
    prover_dal
        .insert_prover_jobs(
            l1_batch_number,
            circuits.clone(),
            AggregationRound::BasicCircuits,
            ProtocolVersionId::latest() as i32,
        )
        .await;

    let prover_jobs_params = GetProverJobsParams {
        statuses: None,
        blocks: Some(std::ops::Range {
            start: l1_batch_number,
            end: l1_batch_number + 1,
        }),
        limit: None,
        desc: false,
        round: None,
    };
    let jobs = prover_dal.get_jobs(prover_jobs_params).await.unwrap();
    assert_eq!(circuits.len(), jobs.len());
}

#[tokio::test]
async fn test_requeue_prover_jobs() {
    let connection_pool = ConnectionPool::test_pool().await;
    let storage = &mut connection_pool.access_storage().await.unwrap();
    let protocol_version = ProtocolVersion::default();
    storage
        .protocol_versions_dal()
        .save_protocol_version_with_tx(protocol_version)
        .await;
    storage
        .protocol_versions_dal()
        .save_prover_protocol_version(Default::default())
        .await;
    let block_number = 1;
    let header = L1BatchHeader::new(
        L1BatchNumber(block_number),
        0,
        Default::default(),
        Default::default(),
        ProtocolVersionId::latest(),
    );
    storage
        .blocks_dal()
        .insert_l1_batch(&header, &[], Default::default(), &[], &[])
        .await
        .unwrap();

    let mut prover_dal = ProverDal { storage };
    let circuits = create_circuits();
    let l1_batch_number = L1BatchNumber(block_number);
    prover_dal
        .insert_prover_jobs(
            l1_batch_number,
            circuits,
            AggregationRound::BasicCircuits,
            ProtocolVersionId::latest() as i32,
        )
        .await;

    // take all jobs from prover_job table
    for _ in 1..=4 {
        let job = prover_dal
            .get_next_prover_job(&[ProtocolVersionId::latest()])
            .await;
        assert!(job.is_some());
    }
    let job = prover_dal
        .get_next_prover_job(&[ProtocolVersionId::latest()])
        .await;
    assert!(job.is_none());
    // re-queue jobs
    let stuck_jobs = prover_dal
        .requeue_stuck_jobs(Duration::from_secs(0), 10)
        .await;
    assert_eq!(4, stuck_jobs.len());
    // re-check that all jobs can be taken again
    for _ in 1..=4 {
        let job = prover_dal
            .get_next_prover_job(&[ProtocolVersionId::latest()])
            .await;
        assert!(job.is_some());
    }
}
