---
title: "Idempotency on Mobile"
slug: idempotency-mobile
categories: ["software-development"]
tags: ["idempotency", "mobile"]
draft: false
---

User bought some stock but the network is slow. There is no immediate feedback, so out of frustration, they tap the **Buy** button several times. A few seconds later, multiple orders are matched.

Have you ever got into this situation?

Disabling the button in the UI helps, but it isn't guarantee. UI state changes happen on main thread and depend on when the run loops get a chance to process them. If the main thread is busy, multiple tap events may be handled before the UI visibly transitions into its disabled state.

And even when the UI behaves perfectly, relying on the client alone is dangerous. This is where **idempotency** comes in.

## Idempotency
From **__Designing Data Intensive Applications__**:

> An idempotent operation is one that you can perform multiple times, and it has the same effect as if you performed it only once.

In our case, what we actually want to protect is the **user's intents**. If the user intends to buy stock once, we don't want multiple taps, retries, or network issues to accidentally turn that into multiple orders.

But isn't that the backend jobs? Yes, but not entirely. The client needs a way to tell the server: "These requests represent the same intent", while the server needs a way to recognize that intent and ensure it is processed only once.

**__Idempotency is an end-to-end contract, not just a backend trick__**

So Who creates the idempotent key and when?

In our case, since we wanted to protect the user intent, it's on the client side. And since it's purpose to protect the user intent, I'm not recommend doing something like
```swift
func buy() async {
    let key = UUID()
}
```
This defeats the purpose. A key is created every time. Instead, I would do something along this:

```swift
struct OrderDTO {
    let idempotencyKey: UUID
    let symbol: String
    let side: Side
    let price: Decimal
    let lot: Int
}

@MainActor
@Observable
class OrderFormViewModel {
    var symbol = ""
    var price: Decimal = 0
    var lot: Int
    var side: Side = .buy

    private(set) var pendingOrder: OrderDTO?

    func submit() async throws {
        try validate()

        let order: OrderDTO
        if let pendingOrder {
            order = pendingOrder
        } else {
            let newOrder = createOrder()
            pendingOrder = newOrder
            order = newOrder
        }

        do {
            let orderService = try await orderService.buy(order)
            pendingOrder = nil
        } catch {
            // retry using the pending order
            throw error
        }

    }

    private func createOrder() -> OrderDTO {
        OrderDTO(
            idempotencyKey: UUID(),
            symbol: symbol,
            price: price,
            side: side,
            lot: lot
        )
    }
}
```

The important is not the UUID itself. It is when that UUID is created and how long we keep it. Once user submits the order, we create an order intent and assign idempotencyKey to it. Any retry with the same intent will reuse the same key.

So even if the user spams the buy button, it will create the order once and then send it to server. The key should only change when the user creates a new intent. For example, after the previous order has completed and they decided to create another one.

> If you wanted to more orderly fashion instead of using UUID, which use UUID version 4, you can find some third pary library that generate UUID version 7 which include timestamp

But generating the key on the client is half of the contract. The server still needs to decide what to do when it receives the same key more than once. For example:
```bash
POST /order/submit
Idempotency-Key: 01DA...

POST /order/submit
Idempotency-Key: 01DA...

POST /order/submit
Idempotency-Key: 01DA...
```

The server should not treated them as three orders. Instead it can store the relationship between the idempotency key and the order being processed

```plaintext
01DA... -> Order #1234 -> Pending
```

When the request arrived with the same key, the server recognized that it has already seen this intent. Rather than creating another order, it can return the existing result:
```json
{
  "order_id": "1234",
  "status": "pending"
}
```
And if the order has already completed:
```json
{
  "order_id": "1234"
  "status": "matched"
  "payload": {
    ...
  }
}
```

Now the responsibility is quite obvious:

**The client declares the intent, and the server guarantees that the intent is not executed twice**

And that is where things get more interesting on the backend: where do we store the key? How long should it live? What happens if two identical requests arrive at exactly the same time?

That's a topic for the next post.
