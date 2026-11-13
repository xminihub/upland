export type HelloResponse = {
  message: string;
};

export async function fetchHello(): Promise<HelloResponse> {
  const response = await fetch("/api/hello");
  if (!response.ok) {
    throw new Error(`Request failed with status ${response.status}`);
  }
  return (await response.json()) as HelloResponse;
}
