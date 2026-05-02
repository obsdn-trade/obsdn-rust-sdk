// Capture deterministic EIP-712 golden fixtures from the Go signer.
//
// Run once when templates or the canonical staging domain change:
//
//	go run ./sdk/rust/scripts/capture_eip712_fixtures.go
//
// Writes one JSON file per template under
// sdk/rust/tests/fixtures/eip712/. Each fixture pins:
//   - the exact domain (so Rust test asserts byte-equal struct hash)
//   - the user-facing input record (so Rust test rebuilds the typed struct)
//   - the EIP-712 digest (the value passed to ecrecover)
//   - the secp256k1 signature with v in {27,28} (matches Go's wire format)
//
// The fixed private key is intentionally dummy: 0x0101...0101.
package main

import (
	"crypto/ecdsa"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math/big"
	"os"
	"path/filepath"
	"strconv"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/common/math"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/signer/core/apitypes"
	"github.com/obsdn-trade/nil/pkg/config"
	"github.com/obsdn-trade/nil/pkg/ethsig"
	"github.com/obsdn-trade/nil/pkg/models"
	"github.com/shopspring/decimal"
)

// Canonical staging domain (base-sepolia) — see configs/shared/chain/base_sepolia.yaml.
// Pinning this here means a domain change requires re-running the script and
// the diff to fixtures is the audit trail.
var stagingDomain = config.Domain{
	Name:              "Obsidian",
	Version:           "1",
	ChainID:           "84532",
	VerifyingContract: "0x988Af38b04a377322aB9A5214F045938348dB155",
}

// Deterministic test key. NOT a real key — published widely in test vectors.
const dummyKeyHex = "0101010101010101010101010101010101010101010101010101010101010101"

// Common deterministic addresses used across fixtures.
var (
	addrSender       = common.HexToAddress("0x1111111111111111111111111111111111111111")
	addrTo           = common.HexToAddress("0x2222222222222222222222222222222222222222")
	addrToken        = common.HexToAddress("0x3333333333333333333333333333333333333333")
	addrVault        = common.HexToAddress("0x4444444444444444444444444444444444444444")
	addrStaker       = common.HexToAddress("0x5555555555555555555555555555555555555555")
	addrSubaccount   = common.HexToAddress("0x6666666666666666666666666666666666666666")
	addrChildAccount = common.HexToAddress("0x7777777777777777777777777777777777777777")
	addrSigner       = common.HexToAddress("0x8888888888888888888888888888888888888888")
)

func main() {
	outDir, err := resolveOutDir()
	must(err)

	priv, err := crypto.HexToECDSA(dummyKeyHex)
	must(err)
	pub := crypto.PubkeyToAddress(priv.PublicKey)

	cases := []fixtureCase{
		orderCase(priv, pub),
		transferCase(priv),
		withdrawCase(priv),
		createVaultCase(priv),
		stakeVaultCase(priv),
		unstakeVaultCase(priv),
		createSubaccountCase(priv),
		registerSenderCase(priv),
		registerSignerCase(priv),
		registerChildSignerCase(priv),
	}

	for _, c := range cases {
		path := filepath.Join(outDir, c.name+".json")
		must(writeFixture(path, c))
		fmt.Printf("wrote %s\n", path)
	}
}

func resolveOutDir() (string, error) {
	// Anchor on the script's own location so `go run` from any CWD writes to
	// the correct fixtures directory.
	wd, err := os.Getwd()
	if err != nil {
		return "", err
	}
	// Walk up until we see go.mod (the repo root), then descend to the
	// fixtures path.
	dir := wd
	for {
		if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
			break
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return "", fmt.Errorf("could not find go.mod from %s", wd)
		}
		dir = parent
	}
	out := filepath.Join(dir, "sdk", "rust", "tests", "fixtures", "eip712")
	if err := os.MkdirAll(out, 0o755); err != nil {
		return "", err
	}
	return out, nil
}

// fixtureCase is the on-disk schema. Field names match Rust serde rename =
// "camelCase" defaults — keep them snake_case here and let serde tolerate it
// (we use serde(rename_all = "snake_case")).
type fixtureCase struct {
	name      string
	template  string
	inputJSON json.RawMessage
	typedData apitypes.TypedData
	signature []byte
	signer    common.Address
}

type fixtureFile struct {
	Template        string          `json:"template"`
	Domain          domainOnDisk    `json:"domain"`
	Input           json.RawMessage `json:"input"`
	DomainSeparator string          `json:"domain_separator"`
	StructHash      string          `json:"struct_hash"`
	Digest          string          `json:"digest"`
	PrivateKey      string          `json:"private_key"`
	SignerAddress   string          `json:"signer_address"`
	Signature       string          `json:"signature"`
}

type domainOnDisk struct {
	Name              string `json:"name"`
	Version           string `json:"version"`
	ChainID           string `json:"chain_id"`
	VerifyingContract string `json:"verifying_contract"`
}

func writeFixture(path string, c fixtureCase) error {
	domainSep, err := c.typedData.HashStruct("EIP712Domain", c.typedData.Domain.Map())
	if err != nil {
		return fmt.Errorf("hash domain: %w", err)
	}
	structHash, err := c.typedData.HashStruct(c.typedData.PrimaryType, c.typedData.Message)
	if err != nil {
		return fmt.Errorf("hash struct: %w", err)
	}
	digest := crypto.Keccak256(append(append([]byte("\x19\x01"), domainSep...), structHash...))

	out := fixtureFile{
		Template: c.template,
		Domain: domainOnDisk{
			Name:              stagingDomain.Name,
			Version:           stagingDomain.Version,
			ChainID:           stagingDomain.ChainID,
			VerifyingContract: stagingDomain.VerifyingContract,
		},
		Input:           c.inputJSON,
		DomainSeparator: "0x" + hex.EncodeToString(domainSep),
		StructHash:      "0x" + hex.EncodeToString(structHash),
		Digest:          "0x" + hex.EncodeToString(digest),
		PrivateKey:      "0x" + dummyKeyHex,
		SignerAddress:   c.signer.Hex(),
		Signature:       "0x" + hex.EncodeToString(c.signature),
	}
	buf, err := json.MarshalIndent(out, "", "  ")
	if err != nil {
		return err
	}
	buf = append(buf, '\n')
	return os.WriteFile(path, buf, 0o644)
}

// --- per-template builders ---

func orderCase(priv *ecdsa.PrivateKey, signer common.Address) fixtureCase {
	order := ethsig.Order{
		Sender:      signer,
		MarketIndex: 7,
		Side:        models.SideBuy,
		Size:        models.AmountX18(decimal.RequireFromString("1500000000000000000")),    // 1.5e18 (1.5 BTC)
		Price:       models.PriceX18(decimal.RequireFromString("65000123456789012345678")), // 65000.12... e18
		Nonce:       1700000000000000000,
	}
	sig, err := ethsig.SignOrder(priv, stagingDomain, order)
	must(err)
	td := buildOrderTypedData(stagingDomain, order)
	in := mustJSON(map[string]any{
		"sender":       order.Sender.Hex(),
		"market_index": order.MarketIndex,
		"side":         "buy",
		"size":         order.Size.Decimal().String(),
		"price":        order.Price.Decimal().String(),
		"nonce":        strconv.FormatUint(order.Nonce, 10),
	})
	return fixtureCase{
		name: "order", template: "Order", inputJSON: in,
		typedData: td, signature: sig, signer: signer,
	}
}

func transferCase(priv *ecdsa.PrivateKey) fixtureCase {
	in := ethsig.TransferSignInput{
		From:   addrSender,
		To:     addrTo,
		Token:  addrToken,
		Amount: bigIntFromString("1000000000"), // 1000 USDC at 6 decimals
		Nonce:  42,
	}
	sig, err := ethsig.SignTransfer(priv, stagingDomain, in)
	must(err)
	td := buildTransferTypedData(stagingDomain, in)
	signer := crypto.PubkeyToAddress(priv.PublicKey)
	return fixtureCase{
		name: "transfer", template: "Transfer",
		inputJSON: mustJSON(map[string]any{
			"from":   in.From.Hex(),
			"to":     in.To.Hex(),
			"token":  in.Token.Hex(),
			"amount": in.Amount.String(),
			"nonce":  strconv.FormatUint(in.Nonce, 10),
		}),
		typedData: td, signature: sig, signer: signer,
	}
}

func withdrawCase(priv *ecdsa.PrivateKey) fixtureCase {
	in := ethsig.WithdrawSignInput{
		Sender: addrSender,
		Token:  addrToken,
		Amount: bigIntFromString("500000000"),
		Nonce:  99,
	}
	sig, err := ethsig.SignWithdraw(priv, stagingDomain, in)
	must(err)
	td := buildWithdrawTypedData(stagingDomain, in)
	signer := crypto.PubkeyToAddress(priv.PublicKey)
	return fixtureCase{
		name: "withdraw", template: "Withdraw",
		inputJSON: mustJSON(map[string]any{
			"sender": in.Sender.Hex(),
			"token":  in.Token.Hex(),
			"amount": in.Amount.String(),
			"nonce":  strconv.FormatUint(in.Nonce, 10),
		}),
		typedData: td, signature: sig, signer: signer,
	}
}

func createVaultCase(priv *ecdsa.PrivateKey) fixtureCase {
	in := ethsig.CreateVaultSignInput{
		Main:           addrSender,
		Vault:          addrVault,
		ProfitShareBps: big.NewInt(2500),
	}
	sig, err := ethsig.SignCreateVault(priv, stagingDomain, in)
	must(err)
	td := buildCreateVaultTypedData(stagingDomain, in)
	signer := crypto.PubkeyToAddress(priv.PublicKey)
	return fixtureCase{
		name: "create_vault", template: "CreateVault",
		inputJSON: mustJSON(map[string]any{
			"main":             in.Main.Hex(),
			"vault":            in.Vault.Hex(),
			"profit_share_bps": in.ProfitShareBps.String(),
		}),
		typedData: td, signature: sig, signer: signer,
	}
}

func stakeVaultCase(priv *ecdsa.PrivateKey) fixtureCase {
	in := ethsig.StakeVaultSignInput{
		Vault:  addrVault,
		Staker: addrStaker,
		Token:  addrToken,
		Amount: bigIntFromString("100000000"),
		Nonce:  17,
	}
	sig, err := ethsig.SignStakeVault(priv, stagingDomain, in)
	must(err)
	td := buildStakeVaultTypedData(stagingDomain, in)
	signer := crypto.PubkeyToAddress(priv.PublicKey)
	return fixtureCase{
		name: "stake_vault", template: "StakeVault",
		inputJSON: mustJSON(map[string]any{
			"vault":  in.Vault.Hex(),
			"staker": in.Staker.Hex(),
			"token":  in.Token.Hex(),
			"amount": in.Amount.String(),
			"nonce":  strconv.FormatUint(in.Nonce, 10),
		}),
		typedData: td, signature: sig, signer: signer,
	}
}

func unstakeVaultCase(priv *ecdsa.PrivateKey) fixtureCase {
	in := ethsig.UnstakeVaultSignInput{
		Vault:  addrVault,
		Staker: addrStaker,
		Token:  addrToken,
		Amount: bigIntFromString("50000000"),
		Nonce:  19,
	}
	sig, err := ethsig.SignUnstakeVault(priv, stagingDomain, in)
	must(err)
	td := buildUnstakeVaultTypedData(stagingDomain, in)
	signer := crypto.PubkeyToAddress(priv.PublicKey)
	return fixtureCase{
		name: "unstake_vault", template: "UnstakeVault",
		inputJSON: mustJSON(map[string]any{
			"vault":  in.Vault.Hex(),
			"staker": in.Staker.Hex(),
			"token":  in.Token.Hex(),
			"amount": in.Amount.String(),
			"nonce":  strconv.FormatUint(in.Nonce, 10),
		}),
		typedData: td, signature: sig, signer: signer,
	}
}

func createSubaccountCase(priv *ecdsa.PrivateKey) fixtureCase {
	in := ethsig.CreateSubaccountSignInput{
		Main:       addrSender,
		Subaccount: addrSubaccount,
	}
	sig, err := ethsig.SignCreateSubaccount(priv, stagingDomain, in)
	must(err)
	td := buildCreateSubaccountTypedData(stagingDomain, in)
	signer := crypto.PubkeyToAddress(priv.PublicKey)
	return fixtureCase{
		name: "create_subaccount", template: "CreateSubaccount",
		inputJSON: mustJSON(map[string]any{
			"main":       in.Main.Hex(),
			"subaccount": in.Subaccount.Hex(),
		}),
		typedData: td, signature: sig, signer: signer,
	}
}

func registerSenderCase(priv *ecdsa.PrivateKey) fixtureCase {
	signerAddr := addrSigner
	nonce := uint64(123)
	message := "I authorize 0x8888888888888888888888888888888888888888 to sign on my behalf."
	sig, err := ethsig.SignSenderMessage(priv, stagingDomain, signerAddr.Hex(), nonce, message)
	must(err)
	td := buildSenderTypedData(stagingDomain, signerAddr.Hex(), nonce, message)
	signer := crypto.PubkeyToAddress(priv.PublicKey)
	return fixtureCase{
		name: "register_signed_by_sender", template: "Register",
		inputJSON: mustJSON(map[string]any{
			"signer":  signerAddr.Hex(),
			"message": message,
			"nonce":   strconv.FormatUint(nonce, 10),
		}),
		typedData: td, signature: sig, signer: signer,
	}
}

func registerSignerCase(priv *ecdsa.PrivateKey) fixtureCase {
	account := addrSender.Hex()
	sig, err := ethsig.SignSignerMessage(priv, stagingDomain, account)
	must(err)
	td := buildSignerTypedData(stagingDomain, account)
	signer := crypto.PubkeyToAddress(priv.PublicKey)
	return fixtureCase{
		name: "register_signed_by_signer", template: "DelegatedSigner",
		inputJSON: mustJSON(map[string]any{
			"account": account,
		}),
		typedData: td, signature: sig, signer: signer,
	}
}

func registerChildSignerCase(priv *ecdsa.PrivateKey) fixtureCase {
	main := addrSender
	child := addrChildAccount
	signerAddr := addrSigner
	nonce := uint64(7)
	message := "child-account-register"
	td := buildChildSignerTypedData(stagingDomain, main, child, signerAddr, message, nonce)
	// No SignRegisterChildAccountSigner exported — sign typed data directly
	// using the canonical signing path.
	sig, err := ethsig.SignTypedData(priv, td)
	must(err)
	signer := crypto.PubkeyToAddress(priv.PublicKey)
	return fixtureCase{
		name: "register_child_account_signer", template: "RegisterChildAccountSigner",
		inputJSON: mustJSON(map[string]any{
			"main":          main.Hex(),
			"child_account": child.Hex(),
			"signer":        signerAddr.Hex(),
			"message":       message,
			"nonce":         strconv.FormatUint(nonce, 10),
		}),
		typedData: td, signature: sig, signer: signer,
	}
}

// --- typed-data builders (mirror unexported helpers in pkg/ethsig) ---
//
// We rebuild the apitypes.TypedData here because Go's signers don't return
// it. Field-for-field copies of the corresponding `build*TypedData` helpers
// in pkg/ethsig — kept in sync by structure, not by import.

func buildOrderTypedData(domain config.Domain, order ethsig.Order) apitypes.TypedData {
	chainID, _ := strconv.ParseInt(domain.ChainID, 10, 64)
	side := uint8(0)
	if order.Side == models.SideSell {
		side = 1
	}
	return apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": []apitypes.Type{
				{Name: "name", Type: "string"},
				{Name: "version", Type: "string"},
				{Name: "chainId", Type: "uint256"},
				{Name: "verifyingContract", Type: "address"},
			},
			"Order": []apitypes.Type{
				{Name: "sender", Type: "address"},
				{Name: "marketIndex", Type: "uint8"},
				{Name: "side", Type: "uint8"},
				{Name: "size", Type: "uint128"},
				{Name: "price", Type: "uint128"},
				{Name: "nonce", Type: "uint64"},
			},
		},
		PrimaryType: "Order",
		Domain: apitypes.TypedDataDomain{
			Name:              domain.Name,
			Version:           domain.Version,
			ChainId:           apitypeChainID(chainID),
			VerifyingContract: domain.VerifyingContract,
		},
		Message: apitypes.TypedDataMessage{
			"sender":      order.Sender.String(),
			"marketIndex": float64(order.MarketIndex),
			"side":        float64(side),
			"size":        order.Size.Decimal().String(),
			"price":       order.Price.Decimal().String(),
			"nonce":       strconv.FormatUint(order.Nonce, 10),
		},
	}
}

func buildTransferTypedData(domain config.Domain, in ethsig.TransferSignInput) apitypes.TypedData {
	chainID, _ := strconv.ParseInt(domain.ChainID, 10, 64)
	return apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": eip712DomainType(),
			"Transfer": []apitypes.Type{
				{Name: "from", Type: "address"},
				{Name: "to", Type: "address"},
				{Name: "token", Type: "address"},
				{Name: "amount", Type: "uint128"},
				{Name: "nonce", Type: "uint64"},
			},
		},
		PrimaryType: "Transfer",
		Domain:      domainStruct(domain, chainID),
		Message: apitypes.TypedDataMessage{
			"from":   in.From.String(),
			"to":     in.To.String(),
			"token":  in.Token.String(),
			"amount": in.Amount.String(),
			"nonce":  strconv.FormatUint(in.Nonce, 10),
		},
	}
}

func buildWithdrawTypedData(domain config.Domain, in ethsig.WithdrawSignInput) apitypes.TypedData {
	chainID, _ := strconv.ParseInt(domain.ChainID, 10, 64)
	return apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": eip712DomainType(),
			"Withdraw": []apitypes.Type{
				{Name: "sender", Type: "address"},
				{Name: "token", Type: "address"},
				{Name: "amount", Type: "uint128"},
				{Name: "nonce", Type: "uint64"},
			},
		},
		PrimaryType: "Withdraw",
		Domain:      domainStruct(domain, chainID),
		Message: apitypes.TypedDataMessage{
			"sender": in.Sender.String(),
			"token":  in.Token.String(),
			"amount": in.Amount.String(),
			"nonce":  strconv.FormatUint(in.Nonce, 10),
		},
	}
}

func buildCreateVaultTypedData(domain config.Domain, in ethsig.CreateVaultSignInput) apitypes.TypedData {
	chainID, _ := strconv.ParseInt(domain.ChainID, 10, 64)
	return apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": eip712DomainType(),
			"CreateVault": []apitypes.Type{
				{Name: "main", Type: "address"},
				{Name: "vault", Type: "address"},
				{Name: "profitShareBps", Type: "uint256"},
			},
		},
		PrimaryType: "CreateVault",
		Domain:      domainStruct(domain, chainID),
		Message: apitypes.TypedDataMessage{
			"main":           in.Main.String(),
			"vault":          in.Vault.String(),
			"profitShareBps": in.ProfitShareBps.String(),
		},
	}
}

func buildStakeVaultTypedData(domain config.Domain, in ethsig.StakeVaultSignInput) apitypes.TypedData {
	chainID, _ := strconv.ParseInt(domain.ChainID, 10, 64)
	return apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": eip712DomainType(),
			"StakeVault": []apitypes.Type{
				{Name: "vault", Type: "address"},
				{Name: "staker", Type: "address"},
				{Name: "token", Type: "address"},
				{Name: "amount", Type: "uint256"},
				{Name: "nonce", Type: "uint64"},
			},
		},
		PrimaryType: "StakeVault",
		Domain:      domainStruct(domain, chainID),
		Message: apitypes.TypedDataMessage{
			"vault":  in.Vault.String(),
			"staker": in.Staker.String(),
			"token":  in.Token.String(),
			"amount": in.Amount.String(),
			"nonce":  strconv.FormatUint(in.Nonce, 10),
		},
	}
}

func buildUnstakeVaultTypedData(domain config.Domain, in ethsig.UnstakeVaultSignInput) apitypes.TypedData {
	chainID, _ := strconv.ParseInt(domain.ChainID, 10, 64)
	return apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": eip712DomainType(),
			"UnstakeVault": []apitypes.Type{
				{Name: "vault", Type: "address"},
				{Name: "staker", Type: "address"},
				{Name: "token", Type: "address"},
				{Name: "amount", Type: "uint256"},
				{Name: "nonce", Type: "uint64"},
			},
		},
		PrimaryType: "UnstakeVault",
		Domain:      domainStruct(domain, chainID),
		Message: apitypes.TypedDataMessage{
			"vault":  in.Vault.String(),
			"staker": in.Staker.String(),
			"token":  in.Token.String(),
			"amount": in.Amount.String(),
			"nonce":  strconv.FormatUint(in.Nonce, 10),
		},
	}
}

func buildCreateSubaccountTypedData(domain config.Domain, in ethsig.CreateSubaccountSignInput) apitypes.TypedData {
	chainID, _ := strconv.ParseInt(domain.ChainID, 10, 64)
	return apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": eip712DomainType(),
			"CreateSubaccount": []apitypes.Type{
				{Name: "main", Type: "address"},
				{Name: "subaccount", Type: "address"},
			},
		},
		PrimaryType: "CreateSubaccount",
		Domain:      domainStruct(domain, chainID),
		Message: apitypes.TypedDataMessage{
			"main":       in.Main.String(),
			"subaccount": in.Subaccount.String(),
		},
	}
}

func buildSenderTypedData(domain config.Domain, signer string, nonce uint64, message string) apitypes.TypedData {
	chainID, _ := strconv.ParseInt(domain.ChainID, 10, 64)
	return apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": eip712DomainType(),
			"Register": []apitypes.Type{
				{Name: "signer", Type: "address"},
				{Name: "message", Type: "string"},
				{Name: "nonce", Type: "uint64"},
			},
		},
		PrimaryType: "Register",
		Domain:      domainStruct(domain, chainID),
		Message: apitypes.TypedDataMessage{
			"signer":  signer,
			"message": message,
			"nonce":   strconv.FormatUint(nonce, 10),
		},
	}
}

func buildSignerTypedData(domain config.Domain, account string) apitypes.TypedData {
	chainID, _ := strconv.ParseInt(domain.ChainID, 10, 64)
	return apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": eip712DomainType(),
			"DelegatedSigner": []apitypes.Type{
				{Name: "account", Type: "address"},
			},
		},
		PrimaryType: "DelegatedSigner",
		Domain:      domainStruct(domain, chainID),
		Message: apitypes.TypedDataMessage{
			"account": account,
		},
	}
}

func buildChildSignerTypedData(
	domain config.Domain,
	main, child, signer common.Address,
	message string,
	nonce uint64,
) apitypes.TypedData {
	chainID, _ := strconv.ParseInt(domain.ChainID, 10, 64)
	return apitypes.TypedData{
		Types: apitypes.Types{
			"EIP712Domain": eip712DomainType(),
			"RegisterChildAccountSigner": []apitypes.Type{
				{Name: "main", Type: "address"},
				{Name: "childAccount", Type: "address"},
				{Name: "signer", Type: "address"},
				{Name: "message", Type: "string"},
				{Name: "nonce", Type: "uint64"},
			},
		},
		PrimaryType: "RegisterChildAccountSigner",
		Domain:      domainStruct(domain, chainID),
		Message: apitypes.TypedDataMessage{
			"main":         main.String(),
			"childAccount": child.String(),
			"signer":       signer.String(),
			"message":      message,
			"nonce":        strconv.FormatUint(nonce, 10),
		},
	}
}

// --- helpers ---

func eip712DomainType() []apitypes.Type {
	return []apitypes.Type{
		{Name: "name", Type: "string"},
		{Name: "version", Type: "string"},
		{Name: "chainId", Type: "uint256"},
		{Name: "verifyingContract", Type: "address"},
	}
}

func domainStruct(d config.Domain, chainID int64) apitypes.TypedDataDomain {
	return apitypes.TypedDataDomain{
		Name:              d.Name,
		Version:           d.Version,
		ChainId:           apitypeChainID(chainID),
		VerifyingContract: d.VerifyingContract,
	}
}

func apitypeChainID(v int64) *math.HexOrDecimal256 {
	return math.NewHexOrDecimal256(v)
}

func bigIntFromString(s string) *big.Int {
	v, ok := new(big.Int).SetString(s, 10)
	if !ok {
		panic("bad bigint literal: " + s)
	}
	return v
}

func mustJSON(v any) json.RawMessage {
	buf, err := json.Marshal(v)
	must(err)
	return buf
}

func must(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "fatal:", err)
		os.Exit(1)
	}
}
