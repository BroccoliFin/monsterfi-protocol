// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "@openzeppelin/contracts/proxy/Clones.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "./MonsterVault.sol";
import "../interfaces/IStrategyConfig.sol";

contract MonsterFactory is Ownable {
    address public implementation;
    
    event MonsterLaunched(
        address indexed vault,
        address indexed token,
        address indexed creator,
        string name,
        bytes32 configHash
    );
    
    event ImplementationUpdated(address indexed oldImpl, address indexed newImpl);
    
    constructor(address _implementation) {
        _setImplementation(_implementation);
    }
    
    function launchMonster(
        address _creator,
        string calldata _name,
        string calldata _symbol,
        bytes32 _configHash,
        IStrategyConfig.StrategyParams calldata _params
    ) external returns (address vault) {
        require(_params.leverage <= MonsterVault.MAX_LEVERAGE, "Leverage too high");
        
        // Деплоим клон
        vault = Clones.clone(implementation);
        
        // Инициализируем
        MonsterVault(vault).initialize(
            _creator,
            _name,
            _symbol,
            _configHash,
            _params
        );
        
        // Эмитим событие
        emit MonsterLaunched(
            vault,
            MonsterVault(vault).address(), // token address = vault address для простоты
            _creator,
            _name,
            _configHash
        );
    }
    
    function _setImplementation(address _newImpl) internal {
        emit ImplementationUpdated(implementation, _newImpl);
        implementation = _newImpl;
    }
    
    function setImplementation(address _newImpl) external onlyOwner {
        _setImplementation(_newImpl);
    }
    
    // Вспомогательная функция для получения адреса токена
    function getTokenAddress(address vault) external pure returns (address) {
        return vault; // В этой реализации токен = вольтом
    }
}
