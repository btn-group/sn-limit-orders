<!-- PROJECT LOGO -->
<br />
<div align="center">
  <a href="https://github.com/btn-group">
    <img src="images/logo.png" alt="Logo" height="80">
  </a>

  <h3 align="center"># Secret Network Limit Order Smart Contract by btn.group</h3>

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
INIT='{ "name": "SSCRT", "symbol": "SSCRT", "decimals": 6, "initial_balances": [{ "address": "secret1mmhhzccndqplwp9juj6z3hy0eaqh4pf395e2my", "amount": "1000000000000000000" }, { "address": "secret1pt9psved7z8hygryv7wyyur64rumys9ugj6n9w", "amount": "1000000000000000000" }], "prng_seed": "RG9UaGVSaWdodFRoaW5nLg==", "config": { "public_total_supply": true, "enable_deposit": true, "enable_redeem": true, "enable_mint": false, "enable_burn": false } }'
secretcli tx compute instantiate $CODE_ID "$INIT" --from a --label "SSCRT" -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt

# Set viewing key for SSCRT
secretcli tx compute execute secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"set_viewing_key": {"key": "DoTheRightThing.", "padding": "BUTT2022."}}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
secretcli tx compute execute secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"set_viewing_key": {"key": "DoTheRightThing.", "padding": "BUTT2022."}}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt

# Init BUTTON (BUTT)
INIT='{ "name": "BUTTON", "symbol": "BUTT", "decimals": 6, "initial_balances": [{ "address": "secret1mmhhzccndqplwp9juj6z3hy0eaqh4pf395e2my", "amount": "2000000000000000000" }, { "address": "secret1pt9psved7z8hygryv7wyyur64rumys9ugj6n9w", "amount": "2000000000000000000" }], "prng_seed": "RG9UaGVSaWdodFRoaW5nLg==", "config": { "public_total_supply": true, "enable_deposit": false, "enable_redeem": false, "enable_mint": false, "enable_burn": false } }'
secretcli tx compute instantiate $CODE_ID "$INIT" --from a --label "BUTT" -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt

# Set viewing key for BUTT
secretcli tx compute execute secret1hqrdl6wstt8qzshwc6mrumpjk9338k0lpsefm3 '{"set_viewing_key": {"key": "DoTheRightThing.", "padding": "BUTT2022."}}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
secretcli tx compute execute secret1hqrdl6wstt8qzshwc6mrumpjk9338k0lpsefm3 '{"set_viewing_key": {"key": "DoTheRightThing.", "padding": "BUTT2022."}}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt

# Init SXMR (MONERO)
INIT='{ "name": "SXMR", "symbol": "SXMR", "decimals": 12, "initial_balances": [{ "address": "secret1mmhhzccndqplwp9juj6z3hy0eaqh4pf395e2my", "amount": "3000000000000000000" }, { "address": "secret1pt9psved7z8hygryv7wyyur64rumys9ugj6n9w", "amount": "3000000000000000000" }], "prng_seed": "RG9UaGVSaWdodFRoaW5nLg==", "config": { "public_total_supply": true, "enable_deposit": false, "enable_redeem": false, "enable_mint": false, "enable_burn": false } }'
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
secretcli query compute query secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"orders": {"address": "secret1mmhhzccndqplwp9juj6z3hy0eaqh4pf395e2my", "key": "DoTheRightThing.", "page": 0, "page_size": 50}}'
```

3. Query activity records

``` sh
secretcli query compute query secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"activity_records": {"key": "DoTheRightThing.", "page": 0, "page_size": 50}}'
```

### Handle functions

1. Register tokens

``` sh
secretcli tx compute execute secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"register_tokens": {"tokens": [{"address": "secret1pt9psved7z8hygryv7wyyur64rumys9ugj6n9w", "contract_hash": "SOMECONTRACTHASH"}], "viewing_key": "SOMEVIEWINGKEY"}' --from a -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

2. Rescue tokens

``` sh
secretcli tx compute execute secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"rescue_tokens":{ "denom": "DENOM", "key": "SOMEKEY", "token_address": "SOMETOKENADDRESS" }}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

3. Update addresses allowed to fill

``` sh
secretcli tx compute execute secret1vjecguu37pmd577339wrdp208ddzymku0apnlw '{"update_addresses_allowed_to_fill": { "addresses_allowed_to_fill": ["SECRETADDRESS"] }}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

4. Cancel order

``` sh
secretcli tx compute execute secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"send": { "recipient": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "amount": "0", "msg": "eyJjcmVhdGVfc2VuZF9yZXF1ZXN0IjogeyJhZGRyZXNzIjogInNlY3JldDFtbWhoemNjbmRxcGx3cDlqdWo2ejNoeTBlYXFoNHBmMzk1ZTJteSIsICJzZW5kX2Ftb3VudCI6ICI1NTU1NTUiLCAiZGVzY3JpcHRpb24iOiAiYXBvY2FseXB0byIsICJ0b2tlbiI6IHsiYWRkcmVzcyI6ICJzZWNyZXQxOHI1c3ptYThobTkzcHZ4Nmx3cGp3eXhydXcyN2UwazU3dG5jZnkiLCAiY29udHJhY3RfaGFzaCI6ICIzNUY1REIyQkM1Q0Q1NjgxNUQxMEM3QTU2N0Q2ODI3QkVDQ0I4RUFGNDVCQzNGQTAxNjkzMEM0QTgyMDlFQTY5In19fQ==" }}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

5. Create order

``` sh
secretcli tx compute execute secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"send": { "recipient": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "amount": "555", "msg": "eyJjcmVhdGVfc2VuZF9yZXF1ZXN0IjogeyJhZGRyZXNzIjogInNlY3JldDFtbWhoemNjbmRxcGx3cDlqdWo2ejNoeTBlYXFoNHBmMzk1ZTJteSIsICJzZW5kX2Ftb3VudCI6ICI1NTU1NTUiLCAiZGVzY3JpcHRpb24iOiAiYXBvY2FseXB0byIsICJ0b2tlbiI6IHsiYWRkcmVzcyI6ICJzZWNyZXQxOHI1c3ptYThobTkzcHZ4Nmx3cGp3eXhydXcyN2UwazU3dG5jZnkiLCAiY29udHJhY3RfaGFzaCI6ICIzNUY1REIyQkM1Q0Q1NjgxNUQxMEM3QTU2N0Q2ODI3QkVDQ0I4RUFGNDVCQzNGQTAxNjkzMEM0QTgyMDlFQTY5In19fQ==" }}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

6. Fill order

``` sh
secretcli tx compute execute secret18vd8fpwxzck93qlwghaj6arh4p7c5n8978vsyg '{"send": { "recipient": "secret1vjecguu37pmd577339wrdp208ddzymku0apnlw", "amount": "555", "msg": "eyJjcmVhdGVfc2VuZF9yZXF1ZXN0IjogeyJhZGRyZXNzIjogInNlY3JldDFtbWhoemNjbmRxcGx3cDlqdWo2ejNoeTBlYXFoNHBmMzk1ZTJteSIsICJzZW5kX2Ftb3VudCI6ICI1NTU1NTUiLCAiZGVzY3JpcHRpb24iOiAiYXBvY2FseXB0byIsICJ0b2tlbiI6IHsiYWRkcmVzcyI6ICJzZWNyZXQxOHI1c3ptYThobTkzcHZ4Nmx3cGp3eXhydXcyN2UwazU3dG5jZnkiLCAiY29udHJhY3RfaGFzaCI6ICIzNUY1REIyQkM1Q0Q1NjgxNUQxMEM3QTU2N0Q2ODI3QkVDQ0I4RUFGNDVCQzNGQTAxNjkzMEM0QTgyMDlFQTY5In19fQ==" }}' --from b -y --keyring-backend test --gas 3000000 --gas-prices=3.0uscrt
```

<p align="right">(<a href="#top">back to top</a>)</p>

<!-- MARKDOWN LINKS & IMAGES -->
<!-- https://www.markdownguide.org/basic-syntax/#reference-style-links -->
[product-screenshot]: images/screenshot.png