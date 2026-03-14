# 🦖 MonsterFi — Mem-Trading Launchpad

> **Launch Your Monster. Pump Your Bags.**

Decentralized launchpad for tokenized trading strategies on Hyperliquid.  
Each strategy is a **PTS-token** (Personal Trading Strategy) with bonding curve + real PnL execution.

## 📚 Documentation

- 📄 [Whitepaper v2.0](./docs/MonsterFi_Whitepaper_v2.0.md)
- 🏗️ [Architecture](./docs/architecture.md)
- 🗺️ [Roadmap](./docs/roadmap.md)

## 🗂️ Project Structure

monsterfi-protocol/
├── contracts/ # Solidity: MonsterFactory, MonsterVault
├── executor/ # Rust: HyperLiquid-Claw based trading agent
├── frontend/ # Next.js 15: No-code strategy builder
├── script/ # Foundry deploy scripts
├── test/ # Forge unit & integration tests
└── docs/ # Whitepaper, specs, diagrams

## 🚀 Quick Start

### Prerequisites
- Foundry (`curl -L https://foundry.paradigm.xyz | bash`)
- Rust (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- Node.js 20+ (`nvm install 20`)

### Smart Contracts 
```bash
cd contracts
forge install
forge build
forge test
```

### Executor (Rust Agent)
```bash
cd executor
cargo build
cargo test
```

### Frontend
```bash
cd frontend
npm install
npm run dev
```

## 🔗 Links
🌐 Testnet: https://app.hyperliquid-testnet.xyz
🔍 Explorer: https://explorer.hyperliquid-testnet.xyz
📚 Docs: https://docs.hyperliquid.xyz

## 🛡️ Security
⚠️ This is experimental software. Use at your own risk.
Keys never leave TEE/sandbox
Position limits & slippage caps enforced
Emergency pause & withdraw mechanisms

## 🤝 Contributing
Fork the repo
Create feature branch (git checkout -b feat/amazing-feature)
Commit changes (git commit -m 'Add amazing feature')
Push to branch (git push origin feat/amazing-feature)
Open Pull Request

## 📜 License
MIT License — see LICENSE file.
Built with ❤️ for degens, creators, and alpha hunters.
