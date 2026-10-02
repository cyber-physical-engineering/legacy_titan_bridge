// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.19;

/**
 * @title Anchor
 * @author James Thornton
 * @notice Append-only store of Merkle roots from batches of legacy transactions
 * @dev The Titan Bridge sidecar submits a root whenever it commits a batch.
 *      One root stands for every transaction hashed into that batch.
 *
 * Architecture:
 * ┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
 * │  COBOL routine  │────▶│  Rust sidecar    │────▶│   This contract │
 * │  (transactions) │     │  (Merkle batch)  │     │  (root storage) │
 * └─────────────────┘     └──────────────────┘     └─────────────────┘
 *
 * Security notes:
 * - Only the designated submitter can add roots
 * - A stored root cannot be changed or removed: the contract has no
 *   update or delete function
 * - Events allow off-chain indexing and verification
 */
contract Anchor {
    // =========================================================================
    // State Variables
    // =========================================================================

    /// @notice Array of all submitted Merkle roots
    /// @dev Append-only array - roots cannot be modified once added
    bytes32[] public dailyRoots;

    /// @notice Metadata for each root submission
    struct RootMetadata {
        uint256 timestamp;      // Block timestamp when submitted
        uint256 leafCount;      // Number of transactions in this batch
        uint256 blockNumber;    // Block number when submitted
    }

    /// @notice Mapping from root hash to its metadata
    mapping(bytes32 => RootMetadata) public rootMetadata;

    /// @notice Address authorized to submit roots
    address public submitter;

    /// @notice Contract owner (can update submitter)
    address public owner;

    /// @notice Whether the contract is paused
    bool public paused;

    // =========================================================================
    // Events
    // =========================================================================

    /// @notice Emitted when a new Merkle root is submitted
    /// @param root The 32-byte Merkle root hash
    /// @param timestamp Block timestamp of submission
    /// @param leafCount Number of transactions in this batch
    /// @param index Position in the dailyRoots array
    event RootSubmitted(
        bytes32 indexed root,
        uint256 timestamp,
        uint256 leafCount,
        uint256 indexed index
    );

    /// @notice Emitted when the submitter address is updated
    event SubmitterUpdated(address indexed oldSubmitter, address indexed newSubmitter);

    /// @notice Emitted when the contract is paused or unpaused
    event PausedStateChanged(bool isPaused);

    /// @notice Emitted when ownership is transferred
    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);

    // =========================================================================
    // Errors
    // =========================================================================

    /// @notice Thrown when caller is not the authorized submitter
    error UnauthorizedSubmitter();

    /// @notice Thrown when caller is not the owner
    error UnauthorizedOwner();

    /// @notice Thrown when contract is paused
    error ContractPaused();

    /// @notice Thrown when root has already been submitted
    error RootAlreadyExists();

    /// @notice Thrown when zero address is provided
    error ZeroAddress();

    /// @notice Thrown when empty root is provided
    error EmptyRoot();

    // =========================================================================
    // Modifiers
    // =========================================================================

    /// @notice Restricts function to authorized submitter
    modifier onlySubmitter() {
        if (msg.sender != submitter) revert UnauthorizedSubmitter();
        _;
    }

    /// @notice Restricts function to contract owner
    modifier onlyOwner() {
        if (msg.sender != owner) revert UnauthorizedOwner();
        _;
    }

    /// @notice Prevents execution when paused
    modifier whenNotPaused() {
        if (paused) revert ContractPaused();
        _;
    }

    // =========================================================================
    // Constructor
    // =========================================================================

    /// @notice Initialize the Anchor contract
    /// @param _submitter Address authorized to submit Merkle roots
    constructor(address _submitter) {
        if (_submitter == address(0)) revert ZeroAddress();
        
        owner = msg.sender;
        submitter = _submitter;
        paused = false;

        emit OwnershipTransferred(address(0), msg.sender);
        emit SubmitterUpdated(address(0), _submitter);
    }

    // =========================================================================
    // Core Functions
    // =========================================================================

    /**
     * @notice Submit a new Merkle root to the ledger
     * @dev Only callable by the authorized submitter when not paused
     * @param root The 32-byte Merkle root hash
     * @param leafCount Number of transactions included in this batch
     */
    function submitDailyRoot(bytes32 root, uint256 leafCount) 
        external 
        onlySubmitter 
        whenNotPaused 
    {
        // Validate input
        if (root == bytes32(0)) revert EmptyRoot();
        
        // Check for duplicate (optional - can be removed for gas savings)
        if (rootMetadata[root].timestamp != 0) revert RootAlreadyExists();

        // Store root
        uint256 index = dailyRoots.length;
        dailyRoots.push(root);

        // Store metadata
        rootMetadata[root] = RootMetadata({
            timestamp: block.timestamp,
            leafCount: leafCount,
            blockNumber: block.number
        });

        // Emit event for off-chain indexing
        emit RootSubmitted(root, block.timestamp, leafCount, index);
    }

    // =========================================================================
    // View Functions
    // =========================================================================

    /**
     * @notice Get the total number of roots stored
     * @return count Number of Merkle roots in the ledger
     */
    function getRootsCount() external view returns (uint256 count) {
        return dailyRoots.length;
    }

    /**
     * @notice Get a root by its index
     * @param index Position in the dailyRoots array
     * @return root The Merkle root at the specified index
     */
    function getRootByIndex(uint256 index) external view returns (bytes32 root) {
        require(index < dailyRoots.length, "Index out of bounds");
        return dailyRoots[index];
    }

    /**
     * @notice Get metadata for a specific root
     * @param root The Merkle root to query
     * @return timestamp When the root was submitted
     * @return leafCount Number of transactions in the batch
     * @return blockNumber Block number of submission
     */
    function getRootMetadata(bytes32 root) 
        external 
        view 
        returns (
            uint256 timestamp,
            uint256 leafCount,
            uint256 blockNumber
        ) 
    {
        RootMetadata memory meta = rootMetadata[root];
        return (meta.timestamp, meta.leafCount, meta.blockNumber);
    }

    /**
     * @notice Check if a root exists in the ledger
     * @param root The Merkle root to check
     * @return exists True if the root has been submitted
     */
    function rootExists(bytes32 root) external view returns (bool exists) {
        return rootMetadata[root].timestamp != 0;
    }

    /**
     * @notice Get the most recent root
     * @return root The latest Merkle root (or zero if none exist)
     * @return timestamp Submission timestamp
     * @return leafCount Number of transactions
     */
    function getLatestRoot() 
        external 
        view 
        returns (
            bytes32 root,
            uint256 timestamp,
            uint256 leafCount
        ) 
    {
        if (dailyRoots.length == 0) {
            return (bytes32(0), 0, 0);
        }
        
        bytes32 latestRoot = dailyRoots[dailyRoots.length - 1];
        RootMetadata memory meta = rootMetadata[latestRoot];
        return (latestRoot, meta.timestamp, meta.leafCount);
    }

    /**
     * @notice Get a range of roots (for pagination)
     * @param startIndex Starting index (inclusive)
     * @param endIndex Ending index (exclusive)
     * @return roots Array of Merkle roots in the range
     */
    function getRootsRange(uint256 startIndex, uint256 endIndex) 
        external 
        view 
        returns (bytes32[] memory roots) 
    {
        require(startIndex < endIndex, "Invalid range");
        require(endIndex <= dailyRoots.length, "End index out of bounds");
        
        uint256 length = endIndex - startIndex;
        roots = new bytes32[](length);
        
        for (uint256 i = 0; i < length; i++) {
            roots[i] = dailyRoots[startIndex + i];
        }
        
        return roots;
    }

    // =========================================================================
    // Admin Functions
    // =========================================================================

    /**
     * @notice Update the authorized submitter address
     * @dev Only callable by the contract owner
     * @param newSubmitter New address authorized to submit roots
     */
    function setSubmitter(address newSubmitter) external onlyOwner {
        if (newSubmitter == address(0)) revert ZeroAddress();
        
        address oldSubmitter = submitter;
        submitter = newSubmitter;
        
        emit SubmitterUpdated(oldSubmitter, newSubmitter);
    }

    /**
     * @notice Pause or unpause the contract
     * @dev Only callable by the contract owner
     * @param _paused New paused state
     */
    function setPaused(bool _paused) external onlyOwner {
        paused = _paused;
        emit PausedStateChanged(_paused);
    }

    /**
     * @notice Transfer ownership of the contract
     * @dev Only callable by the current owner
     * @param newOwner Address of the new owner
     */
    function transferOwnership(address newOwner) external onlyOwner {
        if (newOwner == address(0)) revert ZeroAddress();
        
        address oldOwner = owner;
        owner = newOwner;
        
        emit OwnershipTransferred(oldOwner, newOwner);
    }
}
