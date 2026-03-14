// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

interface IMonsterVault is IERC20 {
    event StrategyExecuted(int256 pnlDelta, uint256 timestamp);
    event LiquidityAdded(address indexed user, uint256 amount, uint256 tokensMinted);
    event LiquidityRemoved(address indexed user, uint256 tokensBurned, uint256 amountReturned);
    event ConfigUpdated(bytes32 indexed oldHash, bytes32 indexed newHash);
    
    function creator() external view returns (address);
    function configHash() external view returns (bytes32);
    function totalPnL() external view returns (int256);
    function getCurrentPrice() external view returns (uint256);
    function buy() external payable returns (uint256 tokensReceived);
    function sell(uint256 tokenAmount) external returns (uint256 ethReturned);
    function updatePnL(int256 delta) external;
}
