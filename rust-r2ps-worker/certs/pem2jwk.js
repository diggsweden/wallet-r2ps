const fs = require('fs');
const jose = require('node-jose');

const pem = fs.readFileSync('../../filestorage/dev/certs/wallet-hsm.crt', 'ascii');

jose.JWK.asKey(pem, 'pem').then(function(result) {
    console.log(JSON.stringify(result.toJSON(true), null, 2)); // true includes private key
});