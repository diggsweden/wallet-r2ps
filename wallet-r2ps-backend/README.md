# wallet-backend-reference

This is the R2PS backend implementation for Wallet.

## Code standard/decision
    * Records are to be used for data class definition as they are inherently immutable
    * RecordBuilder should always be applied and be the recommended, but not mandatory method, for instantiating records.
    * Migration between DTOs should be done in mapper functions.
