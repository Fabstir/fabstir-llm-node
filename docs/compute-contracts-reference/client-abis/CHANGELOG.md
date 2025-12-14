# Client ABIs Changelog

## January 12, 2025 - API Discovery Update

### Updated Files
- `NodeRegistryFAB-CLIENT-ABI.json` - Added API discovery functions
- `JobMarketplaceFABWithS5Deploy-CLIENT-ABI.json` - New marketplace ABI compatible with 5-field Node struct
- `DEPLOYMENT_INFO.json` - Updated with new contract addresses and migration notes
- `README.md` - Added API discovery usage examples and migration guide

### New Contract Addresses
- **NodeRegistryFAB**: `0x2B745E45818e1dE570f253259dc46b91A82E3204` (was `0x87516C13Ea2f99de598665e14cab64E191A0f8c4`)
- **JobMarketplaceFABWithS5Deploy**: `0x3B632813c3e31D94Fd552b4aE387DD321eec67Ba` (was `0x55A702Ab5034810F5B9720Fe15f83CFcf914F56b`)

### New Functions Added to NodeRegistryFAB
- `registerNodeWithUrl(string metadata, string apiUrl)` - Register with API endpoint
- `updateApiUrl(string newApiUrl)` - Update host's API endpoint
- `getNodeApiUrl(address operator)` - Get host's API URL
- `getNodeFullInfo(address operator)` - Returns (operator, stakedAmount, active, metadata, apiUrl)

### Breaking Changes
- NodeRegistry `nodes()` function now returns 5 fields instead of 4
- All JobMarketplace contracts must be updated to handle the 5-field struct

### Migration Required
1. **SDK Developers**: Update to new contract addresses
2. **Existing Hosts**: Call `updateApiUrl()` to add your API endpoint
3. **Client Applications**: Use new discovery functions instead of hardcoded URLs

### Benefits
- Automatic host endpoint discovery
- No more hardcoded API URLs
- Dynamic endpoint updates without re-registration
- Improved decentralization

## January 5, 2025 - Treasury Accumulation

### Features Added
- Treasury fee accumulation for gas savings
- Batch withdrawal functions
- Emergency withdrawal with accumulation protection

## January 4, 2025 - ProofSystem Fix

### Issues Fixed
- Internal verification function call for USDC sessions
- Minimum 64-byte proof requirement enforced