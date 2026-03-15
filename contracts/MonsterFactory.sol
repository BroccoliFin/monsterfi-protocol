// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "@openzeppelin/contracts-upgradeable/proxy/ClonesUpgradeable.sol";
import "@openzeppelin/contracts-upgradeable/access/OwnableUpgradeable.sol";
import "./MonsterVault.sol";
import "../interfaces/IStrategyConfig.sol";

contract MonsterFactory is OwnableUpgradeable {
    address public implementation;
    
    event MonsterLaunched(
        address indexed vault,
        address indexed token,
        address indexed creator,
        string name,
        bytes32 configHash
    );
    
    constructor() { _disableInitializers(); }
    
    function initialize(address _implementation) public initializer {
        __Ownable_init();
        _setImplementation(_implementation);
    }
    
    function launchMonster(
        address _creator,
        string calldata _name,
        string calldata _symbol,
        bytes32 _configHash,
        IStrategyConfig.StrategyParams calldata _params
    ) external returns (address vault) {
        // Hardcode MAX_LEVERAGE check (50)
        require(_params.leverage <= 50, "Leverage too high");
        
        vault = ClonesUpgradeable.clone(implementation);
        
        MonsterVault(payable(vault)).initialize(
            _creator,
            _name,
            _symbol,
            _configHash,
            _params,
            msg.sender
        );
        
        emit MonsterLaunched(vault, vault, _creator, _name, _configHash);
    }
    
    function _setImplementation(address _newImpl) internal {
        implementation = _newImpl;
    }
    
    function setImplementation(address _newImpl) external onlyOwner {
        _setImplementation(_newImpl);
    }
}
