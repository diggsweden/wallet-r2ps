# wallet-backend-reference

This is a repository containing code and decisions that we have agreed upon to use when building the backend services for the wallet.

This repository might or might not be updated once we have our first few services.


## Code standard/decision
    * Records are to be used for data class definition as they are inherently immutable
    * RecordBuilder should always be applied and be the recommended, but not mandatory method, for instantiating records.
    * Migration between DTOs should be done in mapper functions.