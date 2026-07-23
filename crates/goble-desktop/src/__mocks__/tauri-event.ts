export interface Event<T = unknown> {
  event: string;
  id: number;
  payload: T;
  windowLabel: string;
}

export function listen<T>(_event: string, _handler: (event: Event<T>) => void): Promise<() => void> {
  return Promise.resolve(() => {});
}
