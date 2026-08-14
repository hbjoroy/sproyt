const expectedNode = "v24.19.0";
const expectedNpm = "11.17.0";

if (process.version !== expectedNode) {
  throw new Error(`Sproyt frontend requires Node.js ${expectedNode}, found ${process.version}`);
}

const npmVersion = process.env.npm_config_user_agent?.match(/\bnpm\/(\d+\.\d+\.\d+)\b/)?.[1];
if (npmVersion !== expectedNpm) {
  throw new Error(`Sproyt frontend requires npm ${expectedNpm}, found ${npmVersion ?? "an unknown version"}`);
}
