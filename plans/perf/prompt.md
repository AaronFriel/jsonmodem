you will create an execplan to improve the performance of JsonModem.

your plan should

clone the jiter crate into a .gitignored directory in this repo and thoroughly research the crate.

then, create benchmarks derived from streaming_json_large that skip all streaming and just feed in the entire JSON object in one chunk

observe performance differences jsonmodem, jsonmodembuffers, jsonmodemvalues with jiter

then thoroughly research what makes Jiter faster - there are a number of performance tricks it uses to be fast.

you will need to do multiple spikes on this, and be willing to experiment, try a change, run benchmarks and validations, potentially revert that change, and so-on. take an experimentalist approach here, the only right answer is speed and quality. do not compromise on jsonmodem's zero copy approach, however.

then, make jsonmodemvalues as fast as jiter for the simple case where we parse a single medium or large sized json value.
