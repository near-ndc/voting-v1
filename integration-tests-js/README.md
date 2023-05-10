# Integration Tests in JS

## Requirements

- `yarn >= v3.5`
- `node.js >= v18`

## Running

We are using yarn with pnp linker. Make sure you have the latest yarn downloaded in the project:

```shell
yarn set version latest
yarn install
```

Once you installed dependencies, start tests with:

```shell
yarn run test
```

Yarn pnp linker doesn't create `node_modules` directory. To run Node.js with access to package dependencies run:

```shell
yarn node
```

### NPM users

You can also use NPM

```shell
npm install
npm run test
```
