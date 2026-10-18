// Runs once, when the container first starts.
const shop = db.getSiblingDB("shop");
shop.reviews.insertMany(
  Array.from({ length: 30 }, (_, i) => ({ product: i % 3 === 0 ? "Moka pot" : "Grinder", stars: (i % 5) + 1, text: `Review ${i + 1}`, author: { name: i === 0 ? "Ada" : `User ${i}` } })),
);
shop.customers.insertOne({ name: "Ada Lovelace", tags: ["vip"] });
