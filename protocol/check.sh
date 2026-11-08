#!/usr/bin/env bash
# Pinned TLC checks. Downloads are explicit; all output stays outside Git.
set -euo pipefail

protocol_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
version=1.7.4
checksum=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88
if [[ -d /home/james/scratch/mash-artifacts ]]; then
    default_artifacts=/home/james/scratch/mash-artifacts/protocol
else
    default_artifacts=${XDG_CACHE_HOME:-$HOME/.cache}/mash/protocol
fi
artifacts=${MASH_PROTOCOL_ARTIFACTS:-$default_artifacts}
mkdir -p "$artifacts"
artifacts=$(cd -- "$artifacts" && pwd)
repo_dir=$(cd -- "$protocol_dir/.." && pwd)
case "$artifacts/" in
    "$repo_dir/"*) echo 'Protocol artifacts must be outside the checkout' >&2; exit 2 ;;
esac
jar="$artifacts/tools/tla2tools-$version.jar"

verify_jar() {
    [[ -f "$jar" ]] || {
        echo 'TLC is not installed. Run make protocol-tools first.' >&2
        return 1
    }
    printf '%s  %s\n' "$checksum" "$jar" | sha256sum --check --status
}

if [[ ${1:-} == --install-tools ]]; then
    [[ $# == 1 ]] || { echo 'Unexpected install argument' >&2; exit 2; }
    mkdir -p "$artifacts/tools"
    download=$(mktemp "$artifacts/tools/download.XXXXXX")
    trap 'rm -f -- "$download"' EXIT
    curl --fail --location --silent --show-error --retry 3 \
        "https://github.com/tlaplus/tlaplus/releases/download/v$version/tla2tools.jar" \
        --output "$download"
    printf '%s  %s\n' "$checksum" "$download" | sha256sum --check --status
    mv -- "$download" "$jar"
    echo "Installed verified TLC $version: $jar"
    exit 0
fi

for dependency in java timeout sha256sum; do
    command -v "$dependency" >/dev/null || {
        echo "Required command is missing: $dependency" >&2; exit 2;
    }
done
verify_jar || { echo "Missing or incorrect TLC $version jar: $jar" >&2; exit 2; }

models=(ServiceControl Publication BackfillOrdering BackfillReceipts BackfillTarget CoverageMerge SharedViews NativeSilver RetryViews LocalUpgrade LocalWriter LocalGold RuntimeUpgrade MutationAdmission SyncTransfer ProgressiveBackfill PublishFairness CommitCertify SystemdCutover LegacyActivation SilverMigration Records SharedRecords RecallBackfill RecordExtraction)
self_test=false
if [[ ${1:-} == --self-test ]]; then
    [[ $# == 1 ]] || { echo 'Unexpected self-test argument' >&2; exit 2; }
    models=()
    self_test=true
elif [[ $# == 0 ]]; then
    self_test=true
else
    for model in "$@"; do
        case "$model" in
            ServiceControl|Publication|BackfillOrdering|BackfillReceipts|BackfillTarget|CoverageMerge|SharedViews|NativeSilver|RetryViews|LocalUpgrade|LocalWriter|LocalGold|RuntimeUpgrade|MutationAdmission|SyncTransfer|ProgressiveBackfill|PublishFairness|CommitCertify|SystemdCutover|LegacyActivation|SilverMigration|Records|SharedRecords|RecallBackfill|RecordExtraction) ;;
            *) echo "Unknown model: $model" >&2; exit 2 ;;
        esac
    done
    models=("$@")
fi

mkdir -p "$artifacts/runs"
run_dir=$(mktemp -d "$artifacts/runs/check.XXXXXX")
echo "TLC logs and counterexamples: $run_dir"

check() {
    local model=$1 config=$2 expected=${3:-} status=0
    local work="$run_dir/$config"
    mkdir -p "$work/tmp"
    cp -- "$protocol_dir/$model.tla" "$protocol_dir/$config.cfg" "$work/"
    (
        cd -- "$work"
        timeout --kill-after=10s 120s java -Xmx1g -XX:+UseParallelGC \
            -Djava.io.tmpdir="$work/tmp" \
            -cp "$jar" tlc2.TLC -workers 1 -seed 1 -fp 0 \
            -metadir "$work/states" \
            -config "$config.cfg" "$model.tla"
    ) >"$work/output.log" 2>&1 || status=$?
    if [[ -z $expected ]]; then
        if [[ $status != 0 ]] || ! grep -Fq 'Model checking completed. No error has been found.' "$work/output.log"; then
            cat "$work/output.log" >&2
            echo "FAIL $config (exit $status)" >&2
            return 1
        fi
        echo "PASS $config"
        grep -E '^[0-9]+ states generated|depth of the complete state graph' "$work/output.log"
    else
        # A parse error, timeout, deadlock or unrelated invariant is NOT success.
        if [[ $status != 12 ]] || ! grep -Eq "Invariant $expected is violated(\\.| by the initial state:)" "$work/output.log"; then
            cat "$work/output.log" >&2
            echo "FAIL $config: expected invariant $expected (exit $status)" >&2
            return 1
        fi
        echo "EXPECTED COUNTEREXAMPLE $config: $expected"
    fi
}

for model in "${models[@]}"; do
    check "$model" "$model"
    if [[ $model == ServiceControl ]]; then
        check ServiceControl ServiceControlMissingReceipt AtMostOnce
    fi
    if [[ $model == MutationAdmission ]]; then
        check MutationAdmission MutationAdmissionTwoSlots
    fi
done
if $self_test; then
    check BackfillTarget BackfillTargetMissingPreparation CompletedIsAcknowledged
    check LegacyActivation LegacyActivationRollback NoOldBinaryAfterChange
    check RecordExtraction RecordExtractionEarlyCheckpoint CheckpointHasRecords
    check RecordExtraction RecordExtractionStaleSource ReviewedVersionCommitted
    check Records RecordsSplitReceipt CompleteReceipt
    check Records RecordsNoRevisionGuard SingleSuccessor
    check SharedRecords SharedRecordsLostPrior ForgottenStaysErased
    check RecallBackfill RecallBackfillEarlyAck AcknowledgedIncludesRecords
    check RecallBackfill RecallBackfillEarlyPrune PrunedIncludesRecords
    check SilverMigration SilverMigrationNoAdmission WriterExcluded
    check SystemdCutover SystemdCutoverRollback NoLostAcknowledgements
    check SystemdCutover SystemdCutoverReceipts ReceiptsMatchHistory
    check Publication PublicationAckLatest AcknowledgedIsCommitted
    check NativeSilver NativeSilverNoTombstones NoResurrection
    # Restore the historical replay-ordering bug (mash-8qaj).
    check SharedViews SharedViewsReplayClearFirst NeverFalselyCurrent
    check RetryViews RetryViewsChangedPayload CertifiedMatchesSilver
    check RetryViews RetryViewsLostDeletion CertifiedMatchesSilver
    check LocalUpgrade LocalUpgradeRetireFirst RetireRequiresValidation
    check LocalUpgrade LocalUpgradeDatabaseFirst DatabaseHasAttachments
    check RuntimeUpgrade RuntimeUpgradeNoGuard AdmissionChecked
    check MutationAdmission MutationAdmissionReleaseWorker WorkOwnsCapacity
    check MutationAdmission MutationAdmissionUnfair FairOrder
    check LocalWriter LocalWriterCancelledWorker WorkOwnsLock
    check LocalWriter LocalWriterEarlyRelease WorkOwnsLock
    check LocalGold LocalGoldClearFirst CurrentMatchesSilver
    check LocalGold LocalGoldEarlyCertify CurrentMatchesSilver
    # Concurrency regressions found by running four baseline lanes for one node.
    check BackfillOrdering BackfillOrderingSequential
    check BackfillOrdering BackfillOrderingRebaseBug OrderedRevisions
    check BackfillOrdering BackfillOrderingScopeBug UniqueBackfillIdentity
    check BackfillReceipts BackfillReceiptsMixedBug ServerProgressIsReceipt
    check BackfillReceipts BackfillReceiptsOrderBug ServerProgressIsReceipt
    check CoverageMerge CoverageMergeStaleBug CompletedHasCoverage
    # Proposed synchronization contracts, not deliberate mutations of Rust code.
    check SyncTransfer SyncTransferEarlyCheckpoint CheckpointIsDurable
    check SyncTransfer SyncTransferSplitReceipt CommitHasReceipt
    check SyncTransfer SyncTransferEarlyAck PublishedIsReady
    check SyncTransfer SyncTransferExpireCommitted CommittedCannotExpire
    check SyncTransfer SyncTransferChangedRetry TransferIdentity
    check SyncTransfer SyncTransferSplitClose CheckpointIsDurable
    check ProgressiveBackfill ProgressiveBackfillStaleWrite NewestCommittedWins
    check ProgressiveBackfill ProgressiveBackfillResurrection NoResurrection
    check ProgressiveBackfill ProgressiveBackfillEarlyFinish CompleteHasCoverage
    check ProgressiveBackfill ProgressiveBackfillSparseAck JournalRetainsGaps
    check PublishFairness PublishFairnessBug NoOvertake
    check CommitCertify CommitCertifyBug NoStaleContent
fi
