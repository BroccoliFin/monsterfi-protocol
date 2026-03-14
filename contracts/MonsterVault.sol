// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/proxy/Initializable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "../interfaces/IStrategyConfig.sol";
import "../interfaces/IMonsterVault.sol";

contract MonsterVault is Initializable, ERC20, Ownable, ReentrancyGuard, IMonsterVault {
    // === Constants ===
    uint256 public constant CURVE_SCALE = 1000 * 10**18;
    uint256 public constant PLATFORM_FEE_BPS = 500; // 5%
    uint256 public constant MAX_LEVERAGE = 50;
    
    // === State ===
    address public immutable factory;
    address public creator;
    bytes32 public configHash;
    int256 public totalPnL;
    uint256 public basePrice;
    bool public isActive;
    
    IStrategyConfig.StrategyParams public params;
    
    // === Modifiers ===
    modifier onlyFactory() {
        require(msg.sender == factory, "Not factory");
        _;
    }
    
    modifier onlyExecutor() {
        // В продакшене: проверка через TEE-подпись или multisig
        require(msg.sender == creator || msg.sender == factory, "Not executor");
        _;
    }

    constructor(address _factory) ERC20("", "") {
        factory = _factory;
    }

    function initialize(
        address _creator,
        string memory _name,
        string memory _symbol,
        bytes32 _configHash,
        IStrategyConfig.StrategyParams memory _params
    ) public initializer {
        __ERC20_init(_name, _symbol);
        __Ownable_init(_creator);
        
        creator = _creator;
        configHash = _configHash;
        params = _params;
        basePrice = 1 gwei;
        isActive = true;
        
        // Минтим начальные токены за депозит (если есть)
        if (_params.initialDeposit > 0) {
            _mint(_creator, _params.initialDeposit);
        }
    }

    // === Bonding Curve: Buy ===
    function buy() external payable nonReentrant returns (uint256) {
        require(msg.value > 0, "Zero buy");
        require(isActive, "Vault inactive");
        
        uint256 tokensToMint = _calculateBuyAmount(msg.value);
        require(tokensToMint > 0, "Too small amount");
        
        _mint(msg.sender, tokensToMint);
        emit LiquidityAdded(msg.sender, msg.value, tokensToMint);
        return tokensToMint;
    }

    // === Bonding Curve: Sell ===
    function sell(uint256 tokenAmount) external nonReentrant returns (uint256) {
        require(tokenAmount > 0, "Zero sell");
        require(balanceOf(msg.sender) >= tokenAmount, "Insufficient balance");
        
        uint256 ethToReturn = _calculateSellAmount(tokenAmount);
        require(ethToReturn > 0, "Too small amount");
        require(address(this).balance >= ethToReturn, "Insufficient liquidity");
        
        _burn(msg.sender, tokenAmount);
        payable(msg.sender).transfer(ethToReturn);
        emit LiquidityRemoved(msg.sender, tokenAmount, ethToReturn);
        return ethToReturn;
    }

    // === Pricing Logic (Hybrid: Demand + PNL) ===
    function _calculateBuyAmount(uint256 ethAmount) internal view returns (uint256) {
        uint256 price = getCurrentPrice();
        return (ethAmount * 1e18) / price;
    }

    function _calculateSellAmount(uint256 tokenAmount) internal view returns (uint256) {
        uint256 price = getCurrentPrice();
        // Применяем небольшой спред при продаже (2%)
        return (tokenAmount * price * 98) / (1e18 * 100);
    }

    function getCurrentPrice() public view returns (uint256) {
        uint256 supply = totalSupply();
        if (supply == 0) return basePrice;
        
        // Базовая экспоненциальная кривая от supply
        uint256 demandPrice = basePrice + (supply / CURVE_SCALE);
        
        // Буст от PNL (только положительный, с капом 2x)
        if (totalPnL > 0) {
            uint256 pnlBoost = uint256(totalPnL) / 1e18; // конвертим в "коэффициент"
            if (pnlBoost > 2e18) pnlBoost = 2e18; // кап 200%
            return demandPrice * (1e18 + pnlBoost) / 1e18;
        }
        
        return demandPrice;
    }

    // === Executor Functions (вызываются агентом) ===
    function updatePnL(int256 delta) external onlyExecutor {
        totalPnL += delta;
        emit StrategyExecuted(delta, block.timestamp);
    }

    function updateConfig(bytes32 newHash) external onlyOwner {
        emit ConfigUpdated(configHash, newHash);
        configHash = newHash;
    }

    function pause() external onlyOwner {
        isActive = false;
    }

    function unpause() external onlyOwner {
        isActive = true;
    }

    // === Emergency Withdraw (только для создателя, с ограничением) ===
    function emergencyWithdraw(uint256 amount) external onlyOwner {
        require(!isActive, "Must be paused");
        require(amount <= address(this).balance * 10 / 100, "Max 10% per withdraw");
        payable(creator).transfer(amount);
    }

    receive() external payable {}
}
