export type Order = { id: number; total: number };

export const totalOf = (orders: Order[]) => orders.reduce((sum, o) => sum + o.total, 0);
