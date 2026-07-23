export function invoke<T>(_cmd: string, _args?: Record<string, unknown>): Promise<T> {
  return Promise.resolve(undefined as unknown as T);
}
