
help:
	echo This makefile demos some JSON-RPC curls.
	echo You will need to get an alchemy account and put the URL in ALCHEMY_URL.

curl:
	curl -d '{"method": "eth_getLogs", "params": [{"address":"0xb59f67a8bff5d8cd03f6ac17265c550ed8f33907","fromBlock":"0x429d3b","toBlock":"latest","topics":["0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef","0x00000000000000000000000000b46c2526e227482e2ebb8f4c69e4674d262e75","0x00000000000000000000000054a2d42a40f51259dedd1978f6c118a0f0eff078"]}], "id": 1, "jsonrpc": "2.0"}' -X POST -H "Content-Type: application/json"  $(ALCHEMY_URL)

erc20:
	curl -d '{"method": "eth_call", "params": [{"to":"0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "data": "0x313ce567"}], "id": 1, "jsonrpc": "2.0"}' -X POST -H "Content-Type: application/json"  $(ALCHEMY_URL)
