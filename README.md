# Analos Program Validation Repository

This repository contains the source code for all Analos NFT Launchpad programs for validation and auditing purposes.

## 🚀 **Programs Included**

### Core Programs
1. **💰 Price Oracle** - 9dEJ2oK4cgDE994FU9za4t2BN7mFwSCfhSsLTGD3a4ym
2. **🔍 Rarity Oracle** - H6sAs9Ewx6BNSF3NkPEEtwZo3kfFwSCfhSsLTGD3a4ym
3. **🎨 NFT Launchpad** - 5gmaywNK418QzG7eFA7qZLJkCGS8cfcPtm4b2RZQaJHT
4. **🚀 Token Launch** - [Program ID]

### Enhanced Programs
5. **💼 OTC Enhanced**
6. **🎁 Airdrop Enhanced**
7. **⏰ Vesting Enhanced**
8. **🔒 Token Lock Enhanced**
9. **📊 Monitoring System**

## 📁 **Repository Structure**

`
├── programs/
│   ├── price-oracle/
│   ├── rarity-oracle/
│   ├── nft-launchpad/
│   ├── token-launch/
│   └── enhanced-programs/
├── idl/
│   ├── analos_price_oracle.json
│   ├── analos_rarity_oracle.json
│   ├── analos_nft_launchpad.json
│   └── analos_token_launch.json
├── Anchor.toml
├── Cargo.toml
└── README.md
`

## 🔧 **Building Programs**

`ash
# Install Anchor
sh -c ""

# Build all programs
anchor build

# Deploy to devnet
anchor deploy --provider.cluster devnet

# Deploy to mainnet
anchor deploy --provider.cluster mainnet
`

## 🔍 **Program Validation**

Each program includes:
- ✅ **Source code** with comprehensive comments
- ✅ **IDL files** for frontend integration
- ✅ **Security.txt** for responsible disclosure
- ✅ **Test coverage** for critical functions
- ✅ **Documentation** for each instruction

## 🛡️ **Security**

- All programs include security.txt for responsible disclosure
- Contact: support@launchonlos.fun
- Twitter: @EWildn
- Telegram: t.me/Dubie_420

## 📋 **Deployment Status**

| Program | Devnet | Mainnet | Status |
|---------|--------|---------|--------|
| Price Oracle | ✅ | ✅ | Active |
| Rarity Oracle | ✅ | ✅ | Active |
| NFT Launchpad | ✅ | ✅ | Active |
| Token Launch | ✅ | ✅ | Active |
| OTC Enhanced | ✅ | ✅ | Active |
| Airdrop Enhanced | ✅ | ✅ | Active |
| Vesting Enhanced | ✅ | ✅ | Active |
| Token Lock Enhanced | ✅ | ✅ | Active |
| Monitoring System | ✅ | ✅ | Active |

## 🎯 **Frontend Integration**

The frontend uses IDL files to interact with these programs. Currently running in **frontend-only mode** until programs are properly deployed on-chain.

## 📞 **Support**

- **GitHub Issues**: For bug reports and feature requests
- **Security**: support@launchonlos.fun
- **General**: @EWildn on Twitter

---

**⚠️ Important**: This repository is for validation purposes. Always verify program IDs and deployment status before using in production.
