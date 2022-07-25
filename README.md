<!-- PROJECT LOGO -->
<br />
<div align="center">
  <a href="https://github.com/btn-group">
    <img src="images/logo.png" alt="Logo" height="80">
  </a>

  <h3 align="center">Limit Order Smart Contract by btn.group</h3>

  <p align="center">
    Enabling DEFI limit orders on the Secret Network
    <br />
    <br />
    <a href="https://btn.group/secret_network/button_swap">Try it out</a>
  </p>
</div>

<!-- TABLE OF CONTENTS -->
<details>
  <summary>Table of Contents</summary>
  <ol>
    <li>
      <a href="#about-the-project">About The Project</a>
      <ul>
        <li><a href="#built-with">Built With</a></li>
      </ul>
    </li>
    <li>
      <a href="#getting-started">Getting Started</a>
      <ul>
        <li><a href="#prerequisites">Prerequisites</a></li>
        <li><a href="#setting-up-locally">Setting up locally</a></li>
      </ul>
    </li>
    <li><a href="#usage">Usage</a>
      <ul>
        <li><a href="#init">Init</a></li>
        <li><a href="#queries">Queries</a></li>
        <li><a href="#handle-functions">Handle functions</a></li>
      </ul>
    </li>
  </ol>
</details>

<!-- ABOUT THE PROJECT -->
## About The Project

This is a smart contract for btn.group's Button Swap limit order functionality. User is able to set a limit order for any swap combination in a DEFI, permissionless setting without ever having to trust their funds on a centralized exchange.

<p align="right">(<a href="#top">back to top</a>)</p>

### Built With

* [Cargo](https://doc.rust-lang.org/cargo/)
* [Rust](https://www.rust-lang.org/)
* [secret-toolkit](https://github.com/scrtlabs/secret-toolkit)

<p align="right">(<a href="#top">back to top</a>)</p>

<!-- GETTING STARTED -->
## Getting Started

To get a local copy up and running follow these simple example steps.

### Prerequisites

* Download and install secretcli: https://docs.scrt.network/cli/install-cli.html
* Setup developer blockchain and Docker: https://docs.scrt.network/dev/developing-secret-contracts.html#personal-secret-network-for-secret-contract-development

### Setting up locally

Do this on the command line (terminal etc) in this folder.

1. Run chain locally and make sure to note your wallet addresses.

```sh
docker run -it --rm -p 26657:26657 -p 26656:26656 -p 1337:1337 -v $(pwd):/root/code --name secretdev enigmampc/secret-network-sw-dev
```

2. Access container via separate terminal window

```sh
docker exec -it secretdev /bin/bash

# cd into code folder
cd code
```

3. Store contract

```sh
# Store contracts required for test
secretcli tx compute store snip-20-reference-impl.wasm.gz --from a --gas 3000000 -y --keyring-backend test
secretcli tx compute store sn-limit-orders.wasm.gz --from a --gas 3000000 -y --keyring-backend test

# Get the contract's id
secretcli query compute list-code
```

4. Initiate SNIP-20 contracts and set viewing keys (make sure you substitute the wallet and contract addressses as required)

```sh
# Init SNIP-20 (SSCRT)
CODE_ID=1
INIT='{ "name": "SSCRT", "symbol": "SSCRT", "decimals": 6, "initial_balances": [{ "address": "secret10mmsl6m0ws7ux6p0cetczt3k844ndtjj5zjp06", "amount": "1000000000000000000" }, { "address": "secret18kje49yrwlhzgqrlp9dfstx9chktn4arph8k2c", "amount": "1000000000000000000" }], "prng_seed": "RG9UaGVSaWdodFRoaW5nLg==", "config": { "public_total_supply": true, "enable_deposit": true, "enable_redeem": true, "enable_mint": false, "enable_burn": false } }'
secretcli tx compute instantiate $CODE_ID "$INIT" --from a --label "SSCRT" -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt

# Set viewing key for SSCRT
secretcli tx compute execute secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"set_viewing_key": {"key": "DoTheRightThing.", "padding": "BUTT2022."}}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
secretcli tx compute execute secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"set_viewing_key": {"key": "DoTheRightThing.", "padding": "BUTT2022."}}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt

# Init BUTTON (BUTT)
INIT='{ "name": "BUTTON", "symbol": "BUTT", "decimals": 6, "initial_balances": [{ "address": "secret10mmsl6m0ws7ux6p0cetczt3k844ndtjj5zjp06", "amount": "2000000000000000000" }, { "address": "secret18kje49yrwlhzgqrlp9dfstx9chktn4arph8k2c", "amount": "2000000000000000000" }], "prng_seed": "RG9UaGVSaWdodFRoaW5nLg==", "config": { "public_total_supply": true, "enable_deposit": false, "enable_redeem": false, "enable_mint": false, "enable_burn": false } }'
secretcli tx compute instantiate $CODE_ID "$INIT" --from a --label "BUTT" -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt

# Set viewing key for BUTT
secretcli tx compute execute secret1hqrdl6wstt8qzshwc6mrumpjk9338k0lpsefm3 '{"set_viewing_key": {"key": "DoTheRightThing.", "padding": "BUTT2022."}}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
secretcli tx compute execute secret1hqrdl6wstt8qzshwc6mrumpjk9338k0lpsefm3 '{"set_viewing_key": {"key": "DoTheRightThing.", "padding": "BUTT2022."}}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt

# Init SXMR (MONERO)
INIT='{ "name": "SXMR", "symbol": "SXMR", "decimals": 12, "initial_balances": [{ "address": "secret10mmsl6m0ws7ux6p0cetczt3k844ndtjj5zjp06", "amount": "3000000000000000000" }, { "address": "secret18kje49yrwlhzgqrlp9dfstx9chktn4arph8k2c", "amount": "3000000000000000000" }], "prng_seed": "RG9UaGVSaWdodFRoaW5nLg==", "config": { "public_total_supply": true, "enable_deposit": false, "enable_redeem": false, "enable_mint": false, "enable_burn": false } }'
secretcli tx compute instantiate $CODE_ID "$INIT" --from a --label "SXMR" -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt

# Set viewing key for SXMR
secretcli tx compute execute secret18r5szma8hm93pvx6lwpjwyxruw27e0k57tncfy '{"set_viewing_key": {"key": "DoTheRightThing.", "padding": "BUTT2022."}}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
secretcli tx compute execute secret18r5szma8hm93pvx6lwpjwyxruw27e0k57tncfy '{"set_viewing_key": {"key": "DoTheRightThing.", "padding": "BUTT2022."}}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

5. Initialize SN Limit Orders (make sure you substitute the wallet and contract addressses as required)

```sh
# Init SN Limit Orders
CODE_ID=2
INIT='{ "butt": {"address": "secret1hqrdl6wstt8qzshwc6mrumpjk9338k0lpsefm3", "contract_hash": "35F5DB2BC5CD56815D10C7A567D6827BECCB8EAF45BC3FA016930C4A8209EA69"} }'
secretcli tx compute instantiate $CODE_ID "$INIT" --from a --label "Limit orderes | btn.group" -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

<p align="right">(<a href="#top">back to top</a>)</p>

<!-- USAGE EXAMPLES -->
## Usage

You can decode and encode the msg used in the send functions below via https://www.base64encode.org/

### Queries

1. Query config

``` sh
secretcli query compute query secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"config": {}}'
```

2. Query orders

``` sh
secretcli query compute query secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"orders": {"address": "secret10mmsl6m0ws7ux6p0cetczt3k844ndtjj5zjp06", "key": "DoTheRightThing.", "page": 0, "page_size": 50}}'
secretcli query compute query secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"orders": {"address": "secret18kje49yrwlhzgqrlp9dfstx9chktn4arph8k2c", "key": "DoTheRightThing.", "page": 0, "page_size": 50}}'
secretcli query compute query secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"orders": {"address": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "key": "DoTheRightThing.", "page": 0, "page_size": 50}}'
```

3. Query activity records

``` sh
secretcli query compute query secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"activity_records": {"key": "DoTheRightThing.", "page": 0, "page_size": 50}}'
```

### Handle functions

1. Register tokens

``` sh
secretcli tx compute execute secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"register_tokens": {"tokens": [{"address": "secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg", "contract_hash": "35F5DB2BC5CD56815D10C7A567D6827BECCB8EAF45BC3FA016930C4A8209EA69"}, {"address": "secret1hqrdl6wstt8qzshwc6mrumpjk9338k0lpsefm3", "contract_hash": "35F5DB2BC5CD56815D10C7A567D6827BECCB8EAF45BC3FA016930C4A8209EA69"}, {"address": "secret18r5szma8hm93pvx6lwpjwyxruw27e0k57tncfy", "contract_hash": "35F5DB2BC5CD56815D10C7A567D6827BECCB8EAF45BC3FA016930C4A8209EA69"}], "viewing_key": "DoTheRightThing."}}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

2. Create order

``` sh
secretcli tx compute execute secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"send": { "recipient": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "amount": "555", "msg": "eyJjcmVhdGVfb3JkZXIiOiB7ImJ1dHRfdmlld2luZ19rZXkiOiAiRG9UaGVSaWdodFRoaW5nLiIsICJ0b19hbW91bnQiOiAiNTU1IiwgInRvX3Rva2VuIjogInNlY3JldDFocXJkbDZ3c3R0OHF6c2h3YzZtcnVtcGprOTMzOGswbHBzZWZtMyJ9fQ==" }}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt

secretcli tx compute execute secret1hqrdl6wstt8qzshwc6mrumpjk9338k0lpsefm3 '{"send": { "recipient": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "amount": "555555", "msg": "eyJjcmVhdGVfb3JkZXIiOiB7ImJ1dHRfdmlld2luZ19rZXkiOiAiRG9UaGVSaWdodFRoaW5nLiIsICJ0b19hbW91bnQiOiAiNTU1IiwgInRvX3Rva2VuIjogInNlY3JldDE4dmQ4ZnB3eHpjazkzcWx3Z2hhajZhcmg0cDdjNW44OTc4dnN5ZyJ9fQ==" }}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt

secretcli tx compute execute secret1hqrdl6wstt8qzshwc6mrumpjk9338k0lpsefm3 '{"send": { "recipient": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "amount": "555555555", "msg": "eyJjcmVhdGVfb3JkZXIiOiB7ImJ1dHRfdmlld2luZ19rZXkiOiAiRG9UaGVSaWdodFRoaW5nLiIsICJ0b19hbW91bnQiOiAiNTU1IiwgInRvX3Rva2VuIjogInNlY3JldDE4dmQ4ZnB3eHpjazkzcWx3Z2hhajZhcmg0cDdjNW44OTc4dnN5ZyJ9fQ==" }}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

3. Rescue tokens

``` sh
secretcli query account secret10mmsl6m0ws7ux6p0cetczt3k844ndtjj5zjp06
secretcli tx send secret10mmsl6m0ws7ux6p0cetczt3k844ndtjj5zjp06 secret1vjecguu37pmd577339wrdp208ddzymku0apnlw 1000uscrt
secretcli query account secret1vjecguu37pmd577339wrdp208ddzymku0apnlw
secretcli tx compute execute secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"rescue_tokens":{ "denom": "uscrt" }}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

``` sh
secretcli query compute query secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"balance": {"address": "secret10mmsl6m0ws7ux6p0cetczt3k844ndtjj5zjp06", "key": "DoTheRightThing."}}'
secretcli tx compute execute secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"transfer": { "recipient": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "amount": "500000000000000000" }}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
secretcli tx compute execute secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"rescue_tokens":{ "key": "DoTheRightThing.", "token_address": "secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg" }}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
secretcli query compute query secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"balance": {"address": "secret10mmsl6m0ws7ux6p0cetczt3k844ndtjj5zjp06", "key": "DoTheRightThing."}}'
```

4. Update addresses allowed to fill

``` sh
secretcli tx compute execute secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"update_addresses_allowed_to_fill": { "addresses_allowed_to_fill": ["secret10mmsl6m0ws7ux6p0cetczt3k844ndtjj5zjp06", "secret18kje49yrwlhzgqrlp9dfstx9chktn4arph8k2c"] }}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

5. Cancel order

``` sh
secretcli query compute query secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"balance": {"address": "secret18kje49yrwlhzgqrlp9dfstx9chktn4arph8k2c", "key": "DoTheRightThing."}}'
secretcli tx compute execute secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"send": { "recipient": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "amount": "0", "msg": "eyJjYW5jZWxfb3JkZXIiOiB7InBvc2l0aW9uIjogMCB9fQ==" }}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

6. Fill order

``` sh
secretcli tx compute execute secret1hqrdl6wstt8qzshwc6mrumpjk9338k0lpsefm3 '{"send": { "recipient": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "amount": "1", "msg": "eyJmaWxsX29yZGVyIjogeyJwb3NpdGlvbiI6IDB9fQ==" }}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
secretcli tx compute execute secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"send": { "recipient": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "amount": "550", "msg": "eyJmaWxsX29yZGVyIjogeyJwb3NpdGlvbiI6IDJ9fQ==" }}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

7. Borrow and swap

``` sh
secretcli tx compute execute secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"handle_first_hop": { "borrow_amount": "555", "hops": [{"from_token": {"address": "secret1hqrdl6wstt8qzshwc6mrumpjk9338k0lpsefm3", "contract_hash": "35F5DB2BC5CD56815D10C7A567D6827BECCB8EAF45BC3FA016930C4A8209EA69"}, "trade_smart_contract": {"address": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "contract_hash": "1776A0E9E1E74D7382BFF798EBEF5D4CAE012BF465C209BA45059F174684F167"}, "position": 0}, {"from_token": {"address": "secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg", "contract_hash": "35F5DB2BC5CD56815D10C7A567D6827BECCB8EAF45BC3FA016930C4A8209EA69"}, "trade_smart_contract": {"address": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "contract_hash": "1776A0E9E1E74D7382BFF798EBEF5D4CAE012BF465C209BA45059F174684F167"}, "position": 1}], "minimum_acceptable_amount": "10" }}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

<p align="right">(<a href="#top">back to top</a>)</p>

<!-- MARKDOWN LINKS & IMAGES -->
<!-- https://www.markdownguide.org/basic-syntax/#reference-style-links -->
[product-screenshot]: images/screenshot.png