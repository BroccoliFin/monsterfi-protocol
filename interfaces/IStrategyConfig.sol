// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

interface IStrategyConfig {
    struct StrategyParams {
        uint8 leverage;           // 1-50x
        uint16 trailingStopPct;   // 0-3000 = 0-30%
        uint8 positionSizing;     // enum: 0=Fixed, 1=Risk%, 2=Kelly, etc.
        uint8 timeframeHigher;    // 0=M1, 1=M5, 2=M15, 3=H1, 4=H4, 5=D1
        uint8 timeframeLower;
        uint8 assetId;            // Hyperliquid asset index
        uint256 initialDeposit;
        uint8 emissionModel;      // 0=Infinite, 1=Deflation, 2=Fixed
        bytes32 configHash;       // IPFS/Arweave hash of full config
    }
    
    function getParams() external view returns (StrategyParams memory);
    function isActive() external view returns (bool);
}