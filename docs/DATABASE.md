Plan is to update the database scheme in a way that it will support the requirements we have as for the API specs and additional improvements of the deamon.

## Tables
### Orders (`orders`)
- order - String: order identifier provided by the frontend 
- payment_status - Enum: (pending|paid|timed_out). 
- withdrawal_status - Enum: (waiting|failed|completed|forced|none). 
- amount - u128: Order amount 
- currency - String: Currency ticker ("DOT"|"USDC"|...). 
- callback: String: Callback url for frontend order status update 
- payment_account: [u8; 32]: Derived address for this order. 
- recipient: [u8; 32]: Address that will receive the payout once the order is fulfilled. 
- message: String|null: Optional parameter for failed orders.
- payment_page: String|null: Optional parameter for the frontend to redirect to a payment page.
- redirect_url: String|null: Optional parameter for the frontend to redirect once the order is repaid.
- death: u32: Expiry timestamp for the order.

### Transactions (`transactions`)
- order - String: order id to link transaction to order
- chain - String: identifier for the chain where transaction occurred
- block_number - Integer: Block number where the transaction is recorded.
- position_in_block - Integer: Position of the transaction within the block. 
- timestamp - Timestamp: Timestamp of the transaction. 
- transaction_bytes - String: Raw transaction data. 
- sender - String: Address sending the transaction. 
- recipient - String: Address receiving the transaction. 
- amount - Float: transaction amount
- currency: String: Transaction currency 
- status - Enum: Transaction status (pending|finalized|failed).

### Instance Metadata (`instance_info`)
- instance_id - String: instance id randomly generated, happy-octopus or similar shit
- version - String: daemon version (storing it just for consistency with ServerInfo struct)
- debug - String: Debug toggle
- kalatori_remark: String: Environment specific something, can be used for whatever
