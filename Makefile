.PHONY: build test deploy-testnet clean

build:
    forge build --sizes

test:
    forge test -vvv

deploy-testnet:
    @forge script script/DeployVault.s.sol:DeployVault \
      --rpc-url https://rpc.hyperliquid-testnet.xyz \
      --broadcast --verify --legacy \
      -vvvv

clean:
    forge clean