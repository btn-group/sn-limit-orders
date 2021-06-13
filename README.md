# Timelock withdrawal smart contract
Very simple contract to lock a token until a specified time. We used this contract to timelock Buttcoin so that users could verify that we can't dump it etc.

#### The concept is very simple:
* Users send specified token to contract.
* Admin can withdraw specified token after the time specified.

## Example contract
* Holodeck testnet: [secret1ndte0mmp9pevh3xnvax3sgsfa2ew8yxz3e567t](https://secretnodes.com/secret/chains/holodeck-2/contracts/secret1ndte0mmp9pevh3xnvax3sgsfa2ew8yxz3e567t)
* Production: [secret1mvy2hy0ml4ek87vphmq4de03ujw4qggtwtpkaw](https://secretnodes.com/secret/chains/secret-2/contracts/secret1mvy2hy0ml4ek87vphmq4de03ujw4qggtwtpkaw)

## Current limitations/recommendations as per review by [baedrik](https://github.com/baedrik)
1. Because the timelock contract doesn’t do any processing when it receives tokens, you don’t really have to code the Receive function. So you don’t even need to do the RegisterReceive. In fact, the UI doesn’t need to use Send at all, it can just use Transfer to send tokens to the timelock contract. Checking that the env.message.sender matches the expected token contract only prevents someone from calling the Receive function directly from their address. It doesn’t prevent the contract from receiving tokens from other snip20 contracts it doesn’t know about.  That is because those snip20 contracts never call the timelock contract’s Receive because it did not register with those snip20 contracts.  So someone can still accidentally send the wrong snip20 token to the contract and it will never be retrievable. Coding an address check in the Receive function doesn’t prevent that. The reason you check env.message.sender in other contracts is if you need to do different processing depending on which token is received, or if you are preventing a user from trying to spoof the contract into thinking that they sent it tokens by just calling Receive directly. In the case of the timelock contract, even if a user calls Receive directly, since it doesn’t do any processing when it receives tokens, it doesn’t accomplish anything for the user, so it is necessary to check for it.
2. One thing to consider is giving the Withdraw function an optional amount parameter. Some user’s might like to only withdraw some of the balance at a time.
3. Another thing to consider for the timelock contract (although it might make things more complicated than you want) is to give it a handle where you can add a token contract address/hash. And it will iterate through the list of all the snip20s it was told about and display a balance for each.  And the withdraw would iterate through all of them and send the full balance of all tokens. That even enables someone to recover funds if they accidentally send the wrong token.  They just notify the contract of the new token and they could be retrieved (after the timelock transpires)
