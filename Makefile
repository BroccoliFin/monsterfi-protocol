.PHONY: build test deploy clean

build:
	forge build --sizes

test:
	forge test -vvv

deploy-testnet:
	forge script script/Deploy.s.sol --rpc-url hyperliquid_testnet --broadcast -vvvv

clean:
	forge clean
