# MeTube Update Retry Guard Design

## Goal

Prevent Tubemin from repeatedly restarting MeTube for a failed download whose retry budget is already exhausted.

## Root cause

The poller currently requests a MeTube yt-dlp update before calling `claim_metube_retry`. Once the database retry budget is exhausted, the claim returns false, but the update request has already restarted MeTube. The same failed item is seen again on the next poll, producing an update/restart loop.

## Design

For each errored MeTube item, Tubemin will first atomically claim the next retry. Only a successful claim may enter the optional update flow. The update remains best-effort; Tubemin waits for MeTube to become reachable, then submits the retry. Exhausted items perform no update and no retry.

## Verification

Add a regression test around the retry-budget boundary and run the complete Tubemin test suite. No ConvertTube code or deployment configuration changes are required.
