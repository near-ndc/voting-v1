#!/usr/bin/env sh

CTR=voting-v1.ndc-gwg.testnet
GWG=ndc-gwg.testnet

# create empty proposal
near call $CTR creat_proposal '{"start": '$(($(date +%s) + 40))'}' --accountId $GWG

near view $CTR get_proposal '{"proposal": 1}'
