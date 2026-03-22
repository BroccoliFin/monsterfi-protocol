// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "../interfaces/IStrategyConfig.sol";
import "../interfaces/IMonsterVault.sol";

contract MonsterVault is ERC20, Ownable, ReentrancyGuard, IMonsterVault {
    // === Constants ===
    uint256 public constant CURVE_SCALE = 1000 * 10 ** 18;
    uint256 public constant PLATFORM_FEE_BPS = 500;
    uint256 public constant MAX_LEVERAGE = 50;

    // === State ===
    address public immutable factory;
    address public creator;
    address public executor;
    bytes32 public configHash;
    int256 public totalPnL;
    uint256 public basePrice;
    bool public isActive;
    bool public isPaused;

    IStrategyConfig.StrategyParams public params;

    // === Modifiers ===
    modifier onlyFactory() {
        require(msg.sender == factory, "Not factory");
        _;
    }

    modifier onlyExecutor() {
        require(msg.sender == executor || msg.sender == factory, "Not executor");
        _;
    }

    modifier whenNotPaused() {
        require(!isPaused, "Paused");
        _;
    }

    constructor(
        address _factory,
        address _creator,
        string memory _name,
        string memory _symbol,
        bytes32 _configHash,
        IStrategyConfig.StrategyParams memory _params,
        address _executor
    ) ERC20(_name, _symbol) Ownable() {
        factory = _factory;
        creator = _creator;
        executor = _executor;
        configHash = _configHash;
        params = _params;
        basePrice = 1 gwei;
        isActive = true;
        isPaused = false;

        transferOwnership(_creator);

        if (_params.initialDeposit > 0) {
            _mint(_creator, _params.initialDeposit);
        }
    }

    // === Bonding Curve: Buy ===
    function buy() external payable nonReentrant whenNotPaused returns (uint256) {
        require(msg.value > 0, "Zero buy");
        require(isActive, "Vault inactive");

        uint256 price = getCurrentPrice();
        uint256 tokensToMint = (msg.value * 1e18) / price;
        require(tokensToMint > 0, "Too small amount");

        _mint(msg.sender, tokensToMint);
        emit LiquidityAdded(msg.sender, msg.value, tokensToMint, price);
        return tokensToMint;
    }

    // === Bonding Curve: Sell ===
    function sell(uint256 tokenAmount) external nonReentrant whenNotPaused returns (uint256) {
        require(tokenAmount > 0, "Zero sell");
        require(balanceOf(msg.sender) >= tokenAmount, "Insufficient balance");

        uint256 price = getCurrentPrice();
        uint256 sellPrice = (price * 98) / 100;
        uint256 ethToReturn = (tokenAmount * sellPrice) / 1e18;

        require(ethToReturn > 0, "Too small amount");
        require(address(this).balance >= ethToReturn, "Insufficient liquidity");

        _burn(msg.sender, tokenAmount);
        payable(msg.sender).transfer(ethToReturn);
        emit LiquidityRemoved(msg.sender, tokenAmount, ethToReturn, price);
        return ethToReturn;
    }

    // === Hybrid Pricing: Demand + PNL (FIXED) ===
    function getCurrentPrice() public view returns (uint256) {
        uint256 supply = totalSupply();
        if (supply == 0) return basePrice;

        // Базовая цена от спроса
        uint256 demandPrice = basePrice + (supply / CURVE_SCALE);

        // Буст от положительного PNL (в базисных пунктах, кап 100% = 2x)
        if (totalPnL > 0) {
            // PnL в wei → базисные пункты относительно basePrice
            // 1 ETH PnL на базе 1 gwei = огромный буст, поэтому капируем
            uint256 boostBps = uint256(totalPnL) * 10000 / basePrice;
            if (boostBps > 10000) boostBps = 10000; // cap at 100% = 2x multiplier
            return demandPrice * (10000 + boostBps) / 10000;
        }

        // Hard floor: -50% от demand price при убытках
        if (totalPnL < 0) {
            uint256 floorPrice = demandPrice / 2;
            return demandPrice > floorPrice ? demandPrice : floorPrice;
        }

        return demandPrice;
    }

    // === Executor Functions ===
    function updatePnL(int256 delta, bytes32 tradeId) external onlyExecutor {
        totalPnL += delta;
        emit StrategyExecuted(delta, block.timestamp, tradeId);
    }

    function updateExecutor(address newExecutor) external onlyOwner {
        emit ExecutorUpdated(executor, newExecutor);
        executor = newExecutor;
    }

    // === Safety ===
    function pause() external onlyOwner {
        isPaused = true;
    }

    function unpause() external onlyOwner {
        isPaused = false;
    }

    function emergencyWithdraw(uint256 amount) external onlyOwner {
        require(isPaused, "Must be paused");
        require(amount <= address(this).balance * 10 / 100, "Max 10%");
        payable(creator).transfer(amount);
    }

    // === Views ===
    function getVaultStats()
        external
        view
        returns (uint256 price, uint256 supply, int256 pnl, bool active, string memory asset)
    {
        string memory assetName =
            params.assetId == 0 ? "BTC" : params.assetId == 1 ? "ETH" : params.assetId == 2 ? "SOL" : "ALT";
        return (getCurrentPrice(), totalSupply(), totalPnL, isActive && !isPaused, assetName);
    }

    receive() external payable {}
}
