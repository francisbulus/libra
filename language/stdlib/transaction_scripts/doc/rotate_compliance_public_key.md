
<a name="SCRIPT"></a>

# Script `rotate_compliance_public_key.move`

### Table of Contents

-  [Function `rotate_compliance_public_key`](#SCRIPT_rotate_compliance_public_key)



<a name="SCRIPT_rotate_compliance_public_key"></a>

## Function `rotate_compliance_public_key`

Encode a program that rotates
<code>account</code>'s compliance public key to
<code>new_key</code>.


<pre><code><b>public</b> <b>fun</b> <a href="#SCRIPT_rotate_compliance_public_key">rotate_compliance_public_key</a>(account: &signer, new_key: vector&lt;u8&gt;)
</code></pre>



<details>
<summary>Implementation</summary>


<pre><code><b>fun</b> <a href="#SCRIPT_rotate_compliance_public_key">rotate_compliance_public_key</a>(account: &signer, new_key: vector&lt;u8&gt;) {
    <a href="../../modules/doc/DualAttestation.md#0x1_DualAttestation_rotate_compliance_public_key">DualAttestation::rotate_compliance_public_key</a>(account, new_key)
}
</code></pre>



</details>
