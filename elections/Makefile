include ../Makefile-common.mk


deploy-testnet:
	near deploy voting-v1.ndc-gwg.testnet ../res/ndc_voting.wasm new \
	  '{"gwg": "ndc-gwg.testnet", "stake_verifier": "stake-verifier.ndc-gwg.testnet", "sup_consent": {"quorum": 8, "threshold": 60}, "consent": {"quorum": 5, "threshold": 50}}'

re-deploy-testnet:
	near deploy voting-v1.ndc-gwg.testnet ../res/ndc_voting.wasm
