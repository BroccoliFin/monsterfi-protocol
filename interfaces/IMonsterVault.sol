// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

interface IMonsterVault is IERC20 {
    event StrategyExecuted(int256 pnlDelta, uint256 timestamp, bytes32 tradeId);
    event LiquidityAdded(address indexed user, uint256 amount, uint256 tokensMinted, uint256 price);
    event LiquidityRemoved(address indexed user, uint256 tokensBurned, uint256 amountReturned, uint256 price);
    event ExecutorUpdated(address indexed oldExecutor, address indexed newExecutor);
    
    function creator() external view returns (address);
    function executor() external view returns (address);
    function configHash() external view returns (bytes32);
    function totalPnL() external view returns (int256);
    function getCurrentPrice() external view returns (uint256);
    function buy() external payable returns (uint256 tokensReceived);
    function sell(uint256 tokenAmount) external returns (uint256 ethReturned);
    function updatePnL(int256 delta, bytes32 tradeId) external;
    function updateExecutor(address newExecutor) external;
    function pause() external;
    function unpause() external;
    function emergencyWithdraw(uint256 amount) external;
    function getVaultStats() external view returns (
        uint256 price,
        uint256 supply,
        int256 pnl,
        bool active,
        string memory asset
    );
}
