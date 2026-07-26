/**
 * Basic example of using the FaniLab TypeScript SDK
 */

import { EscrowClient, DeliveryClient } from '../src/index';

async function main() {
  // Initialize clients with contract addresses
  const escrowContractId = 'CABC...'; // Replace with actual contract ID
  const deliveryContractId = 'CBDE...'; // Replace with actual contract ID

  const escrow = new EscrowClient(escrowContractId);
  const delivery = new DeliveryClient(deliveryContractId);

  try {
    // Example 1: Create a delivery
    console.log('Creating delivery...');
    const deliveryId = await delivery.createDelivery({
      sender: 'GA7VQKQ...',
      recipient: 'GB3UWF4...',
      deliveryId: BigInt(1),
      metadata: {
        pickupLocation: '123 Main St, City',
        dropoffLocation: '456 Oak Ave, City',
        items: 'Package containing books',
        notes: 'Deliver between 9 AM - 5 PM',
        estimatedDistance: 25,
      },
    });
    console.log('Delivery created with ID:', deliveryId);

    // Example 2: Create an escrow for the delivery
    console.log('Creating escrow...');
    await escrow.createEscrow({
      sender: 'GA7VQKQ...',
      recipient: 'GB3UWF4...',
      driver: 'GC5YNXJ...',
      deliveryId: BigInt(1),
      token: 'CCDE...', // USDC or other token
      amount: BigInt(1_000_000_000), // 100 units (7 decimals)
      fleetId: BigInt(1),
    });
    console.log('Escrow created for delivery 1');

    // Example 3: Get escrow status
    console.log('Getting escrow status...');
    const escrowRecord = await escrow.getEscrow(BigInt(1));
    console.log('Escrow status:', escrowRecord.status);
    console.log('Escrow amount:', escrowRecord.amount.toString());

    // Example 4: Assign driver to delivery
    console.log('Assigning driver to delivery...');
    await delivery.assignDriver({
      caller: 'GA7VQKQ...', // Admin or authorized caller
      deliveryId: BigInt(1),
      driver: 'GC5YNXJ...',
    });
    console.log('Driver assigned');

    // Example 5: Mark delivery as in transit
    console.log('Marking delivery as in transit...');
    await delivery.markInTransit({
      caller: 'GC5YNXJ...', // Driver
      deliveryId: BigInt(1),
    });
    console.log('Delivery marked as in transit');

    // Example 6: Confirm delivery completion
    console.log('Confirming delivery completion...');
    await delivery.confirmDelivery({
      caller: 'GB3UWF4...', // Recipient
      deliveryId: BigInt(1),
    });
    console.log('Delivery confirmed');

    // Example 7: Release escrow to driver
    console.log('Releasing escrow to driver...');
    await escrow.releaseEscrow({
      caller: 'GB3UWF4...', // Recipient or admin
      deliveryId: BigInt(1),
    });
    console.log('Escrow released to driver');

    // Example 8: Verify final escrow status
    console.log('Verifying final escrow status...');
    const finalEscrow = await escrow.getEscrow(BigInt(1));
    console.log('Final escrow status:', finalEscrow.status);

    console.log('\n✅ All operations completed successfully!');
  } catch (error) {
    console.error('❌ Error:', error);
    process.exit(1);
  }
}

// Handle dispute scenario
async function handleDispute() {
  const escrow = new EscrowClient('CABC...');

  try {
    console.log('Raising dispute on delivery 1...');
    await escrow.raiseDispute({
      caller: 'GA7VQKQ...', // Sender, recipient, or driver
      deliveryId: BigInt(1),
    });
    console.log('Dispute raised');

    // Wait for admin resolution
    console.log('Waiting for admin resolution...');

    // Scenario 1: Admin releases to driver
    console.log('Admin releasing funds to driver...');
    await escrow.resolveDispute({
      caller: 'GA7VQKQ...', // Admin
      deliveryId: BigInt(1),
      releaseToDriver: true,
    });

    // Scenario 2: Admin refunds to sender
    // await escrow.resolveDispute({
    //   caller: 'GA7VQKQ...', // Admin
    //   deliveryId: BigInt(1),
    //   releaseToDriver: false,
    // });

    // Scenario 3: Admin splits funds
    // await escrow.resolveDisputeSplit({
    //   caller: 'GA7VQKQ...', // Admin
    //   deliveryId: BigInt(1),
    //   senderShareBps: 5000, // 50% to sender, 50% to driver
    // });

    console.log('Dispute resolved');
  } catch (error) {
    console.error('Error handling dispute:', error);
  }
}

// Run the example
main();
