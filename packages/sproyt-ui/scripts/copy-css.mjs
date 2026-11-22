import { readFileSync, writeFileSync } from 'node:fs';
// Local Windows packages must match the byte-for-byte Linux CI build.
writeFileSync('dist/styles.css', readFileSync('src/styles.css', 'utf8').replace(/\r\n/g, '\n'));
writeFileSync('dist/kretsrom-reference.css', readFileSync('src/kretsrom-reference.css', 'utf8').replace(/\r\n/g, '\n'));
