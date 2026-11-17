# Timber time and cost estimates

`wood.estimate` is read-only. Open **Time and Cost Estimate**, or calculate an estimate in the Cutting Plan panel.

The command accepts the same `library`, `stock`, `kerf` (3.2 mm) and `trim` (10 mm) settings as `cutlist.layout`. It counts purchased stock pieces once; remaining offcuts incur no additional charge. Optional `finishing` supplies in the stock JSON have `name`, `quantity` and `unit_price`. Design metadata can supply the same list under `io.solidcraft.wood.estimate`, as an object with a `finishing` array; it takes precedence over the library. The command's `finishing` array overrides either source, including an empty array to clear it.

`rates` names a shared JSON file, independent of the design. Copy the shipped `assets/wood/rates.json` to customise it. All time rates are minutes: `saw_cut_minutes` per cut in the cutting plan, `router_minutes_per_metre` per metre of router edge shape (the exact edge lengths `wood.edge_profile` reports) and `glue_up_minutes` per glue-up. These are estimating allowances, not a toolpath simulation. Enter `glue_ups` (default 0) for actual workshop assembly batches. A non-null `labour_per_hour` enables a separate labour line. Use the same currency for every price and hourly rate.

```json
{"library":"workshop-stock.json","rates":"workshop-rates.json","glue_ups":2,"path":"stool-estimate.csv"}
```

For supplies saved with a design, use the existing metadata command:

```json
{"command":"document.metadata.set","params":{"key":"io.solidcraft.wood.estimate","value":{"finishing":[{"name":"Oil","quantity":0.25,"unit_price":12}]}}}
```

The result contains `stock_cost`, `material_cost` (stock plus finishing), `finishing_cost`, `labour_cost`, `total_cost`, `work_minutes`, `lines`, `warnings` and `complete`. Each line reports its quantity, unit time, time and optional price. Quantities are stock pieces, supply units, saw cuts, router metres, glue-up batches or labour hours, according to the line category. CSV includes the breakdown, totals and warnings. Missing prices, unplaced parts, excluded non-stock geometry and failed or stale features mark the estimate incomplete. Totals then include known costs only. Purchased and reference components are excluded from workshop work just as they are from the cutting plan. Component occurrences multiply feature work; suppressed features and the router's `none` profile have no work time. Costs and plans are recalculated and never stored in the design.
